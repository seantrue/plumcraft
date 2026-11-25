# Bugs

Bugs reported in passing (`bug: …`), kept until fixed. Numbers are never reused. Entries with an issue are also tracked on GitHub.

## Open

### B168. Mac: the offscreen picture of the document window comes out upside down

- Reported: 2026-11-25 (found by an AI agent's launch, not by the maintainer)
- Report: the scripted Mac copy's picture of the whole document window, taken over the control port, is flipped top to bottom: node text reads mirrored vertically and titles sit at the bottom of each node. Pop-out pictures are the right way up.
- Context: views slice 10's launch picture of four node faces. Not known: whether an earlier build's document picture was the right way up, and whether only the picture is flipped or anything a person sees.
- Issue: none

### B167. Mac: the same error twice in a row shows no banner the second time

- Reported: 2026-11-24 (found by an agent while fixing B165, not by the maintainer)
- Report: the Mac shows the document's last error as a banner only when its text differs from the last one shown (`GraphDocument.followState`). A second try at the same failing action, such as inserting the same refused file again, shows nothing at all. Likely why B165 looked silent.
- Issue: none

### B164. Mac: a node driven by a range does not give several results on its output

- Reported: 2026-11-24
- Report: "Node driven by range does not generate multiple results on output"
- Context: the maintainer's first hand use of B162's fix, in an application built on the Mac shell from the dev branch at `3403749`, with the style graph of B162's report: a range driver of 5 values wired to a setting of a node of the application's own engine. The graph now runs.
- Answer (the maintainer, 2026-11-24): asked which output, they answered: "I looked at right after the node, and the final result." So: one result both just after the driven node and at the end of the graph.
- Then: "fix B164".
- Found (an agent, a scripted copy with a scratch copy of the graph; no fix made):
  - The driven node runs 5 times, once per value. Each run returns a list of 2 pictures, and the engine joins the runs into one flat list of 10 (seen: the odd items differ with the value).
  - The wire out of the node carries the slice `1`, which takes one item of that flat list: the first run's picture. From there to the final result there is one picture.
  - This is the list rule as written, not a fault in one place: a node run once per item whose runs each return a list gives "a new, flat list" (`crates/engine/src/exec.rs`; the Python SDK and the research software's executor the same), and a slice applies to the flat list (`docs/interface.md` 7.6 rule 3). The research software's own executor, read and not run, would give one result for this wiring too.
  - Changing the slice to `1::2` on the wire gave 5 items just after the node, but a later node that takes a whole list of 3 then got 15 and refused. So slicing per run is not enough.
  - Ruled out: the border between engines (the 5 values arrive), the inherited mark, dotted names.
- Open question (the maintainer's decision; it changes the list rule in the spec, both SDKs and the research software): keep each run's list apart (nested lists), slice per run only, keep today's rule and say what it does, or have the core run the downstream part once per value.
- Decision (the maintainer, 2026-11-24): asked which of the four, and whether to do the message of the third first: "Option 1, do the option 3 message first". So: nested lists (each run's list kept apart) is the fix; before it, the wire or face says what today's rule does.
- Design: `docs/research/27-nested-lists.md`. Its nine decisions, answered by the maintainer on 2026-11-24: "Decisions: 1B; 2C; 3B; 4A; 5A; 6A; 7A; 8B; 9A" (the interim message is a plain note with no new field; the rule for every graph, with an engine capability; a slice means the level its wire's source made; a whole-list port runs once per outer item; a Flatten building block; two steppers in pop-outs; an axes field on lists; tuples stay lists; the application's engine and the research software change together, last).
- Interim (2026-11-24, branch `fix/b164-range-items`; the bug stays open until nested lists): after a run the core says what today's rule does, with no new message field and no numbers (it is not told a run count). On a wire whose index slice takes one item from a list output of a step that ran once per item: "Slice 1 takes one item. <Step> runs once per item of a list and joins every run's list into one, so this is one run's result, not one from each run (B164)." On an unsliced wire into an input that takes a whole list: "This input takes a whole list, and gets <Step>'s runs joined into one list, not a list per run (B164)." Shown in the wire's context menu and at the end of the input row's hover; the Mac canvas now shows an input's hover as its tooltip, as Windows does. Quiet for an ordinary list, a range slice, and before a run. Read back from the application on the Mac on a scratch copy of the report's graph; the menu line and tooltip have not been seen in a window.
- Slice 1 of note 27 (2026-11-24, branch `feat/nested-lists-s1`): the nested-list rules are in `docs/interface.md` (4.5, the capability `lists.nested`, 7.6 rules 2 and 3), each marked "specified, not built"; a list value gains an optional `axes` field; `conformance/graphs/` starts with 11 vectors, each holding today's result and the nested one, run by `crates/coordinator/tests/vectors.rs`. No engine's behaviour changes. Five points the decisions did not settle are marked "not yet decided" in the spec.
- Slices 2 to 4 of note 27 (2026-11-24, overnight, branch `pr/nested-s2-4`; built and tested, not launched): the Rust and Python SDKs keep each run's list (capability `lists.nested`), slices apply inside a run, a whole-list port runs once per outer item, Flatten (`seq.flatten`); the coordinator refuses a run or a save with a wire between an engine that lists the capability and one that does not; a watch and `run.item` take the item within the run (`path`), and a display gives the count at each level. The interim note now shows only while the connected engine lacks the capability. The eight open points of slice 1 and four new ones are taken as assumed, marked so in the note and the spec. Until slice 6, an application whose own engine lacks the capability has its runs across to the public engines refused.
- Slices 5 and 6 of note 27 (2026-11-25, overnight, branch `pr/nested-s5-6`): the shells step a list of lists with two steppers, runs and the items inside the run shown, and a face shows the count per level ("3 × 2 items"): egui, the Apple shells (launched on a scripted Mac copy and iPad and iPhone simulators), Windows (built on the test VM, not launched). An engine that runs graphs with its own core lists `lists.nested` only when it declares it. With the matching private changes to the application's engine and both of the research software's executors (local, uncommitted), the graph of the report, a scratch copy run in the application on the Mac, gives 5 items after the slice and 5 at the final result; the controller measured the 5 results as differing in about 1.5% of pixels between the first and last value. Not seen in a window: the application's screen was off, so the items were saved as pictures from the engine's output. Stays open until the maintainer confirms the assumptions and the private changes are committed.
- Slice 7 (2026-11-25, overnight, branch `pr/nested-s7`): the rule the report's graph needed and note 27 had wrong is in both SDKs and the spec, marked assumed: a whole-list input that brings a sweep its sibling whole-list inputs do not share is regrouped by that sweep. Vector 11 holds the report's wiring as the graph has it. Thirteen vectors; the Rust SDK runs all, the Python SDK and the research software's two executors run 11 each, and all four agree on every vector they run.
- Decided (2026-11-25): The maintainer, 2026-11-25, on the checklist page of 29 points (cards 1 to 15 are N1 to N15 here, 16 and 17 the release and merge points R1 and R2, 18 to 29 are V1 to V12 of note 26): "Decisions: 1A; 2A; 3A; 4A; 5A; 6A; 7A; 8A; 9A; 10A; 11A; 12A; 13A; 14A; 15A; 16A; 17A; 18A; 19A; 20A; 21A; 22A; 23A; 24A; 25A; 26A; 27A; 28A; 29B". The nested-list points (1 to 15) stand as built; 16A, the next release is 0.8.0 with the grown types marked `non_exhaustive`; 17A, slices 2 to 7 merge together once the private half is committed.
- Slice 8 (2026-11-25, branch `pr/nested-s8`): the confirmed rules lose their assumed marks; the seven public types that gained fields are `non_exhaustive` with constructors (the maintainer's 16A); merged with the views work.
- Issue: none

### B163. A graph's banner keeps "the graph has N problem(s)" after the missing engine was added

- Reported: 2026-11-24 (found by an agent while reproducing B162, not by the maintainer)
- Report: opening a style graph of the research software in an application built on the Mac shell shows "the graph has 13 problem(s)… unknown kind …" for nodes of the array engine. The array engine is then added automatically and those nodes run, but the banner stays, out of date.
- Issue: none

### B157. `bytes_held_elsewhere_are_shared_not_read_again` failed once on the Linux test host

- Reported: 2026-11-24 (found by the controller in a gate run, not by the maintainer)
- Report: the Linux gate on the B153 commit (`d4650db`, which changes no media code) failed `plumcraft-media service::tests::bytes_held_elsewhere_are_shared_not_read_again` at `crates/media/src/service.rs:1317`: "let go of everywhere: read again". The same test passed in the gate run on `eaec253` an hour or so earlier.
- Context: the test drops its two holds on a value's bytes and expects the next `get` to be pending. Its resolver has no delay. Not known: whether something else can still hold the bytes for a moment (a worker's own reference), or whether the `get` can be answered at once. No cause is confirmed; watch for a second failure, as with B144.
- Issue: none

### B154. Native shells: the views of an array have regressed from the egui editor (plots, view as image, heatmap); tables need selected columns visualized

- Reported: 2026-11-24
- Report: "regression in views of arrays (the egui version supported plots, view as image, heatmap, etc). These are important. For tabular data, visualizing selected columns is also important."
- Context: the maintainer's hand use of an application built on the Mac shell, just after B152 (a histogram, a 256×2 array marked as a table, which the native shells show only as a Table of numbers). Known from reading for B152, not a full survey: egui's viewer registry (`crates/ui/src/viewers/mod.rs`) has an `array.plot` viewer ("Plot") that the core's list of offered viewers (`crates/editor-core/src/viewers.rs`, `offered`) leaves out, and the core's own test of the two lists excludes `mesh` and `array.plot` by name; the Apple shells draw `picture`, `svg`, `array.pictures`, `array.grid`, `audio`, `table.grid`, `text.plain`, `scalar`, `yaml`, `tree`, `items` and `hex` (`NATIVE`).
- Open question: the full list of what is missing has not been made. A survey of egui's array and table views against each native shell (Mac, iPad, iPhone, Windows) comes first; then, for tables, how columns are selected for a plot (egui picks them by name: `x`/`y`, or the first two) and which kinds of plot are wanted.
- Direction (the maintainer, 2026-11-24): asked whether to survey first and how columns should be chosen, they answered: "B154: Start this as a parallel research task. Survey first. Look at seaborn. Histograms. Line charts, stacked line charts, column charts, stacked column charts,  treemaps. Interactive is good (selecting columns, rows, etc; for interrogating data, zooming into ranges)". The research is a note in `docs/research/`; nothing is built until it is read.
- Research: `docs/research/26-data-views.md` (2026-11-24): 11 of egui's 17 array and table views are missing on every native shell; the gap is in the core.
- Decisions (the maintainer, 2026-11-24): given the note's seven decisions with a recommendation for each, they answered: "Yes to all". So: (1) the core lays each chart out and each shell paints it; (2) the regressions first, then the named chart kinds, then interaction; (3) a chart's choices are saved with the graph, undoable; (4) egui paints the same chart list and its own plot is retired; (5) Win2D on Windows; (6) the note's ranking of the optional kinds; (7) a selection belongs to the value, shared by its table and chart windows.
- Slice 2 of note 26 (2026-11-24, branch `pr/views-s2`; Mac, iPad, iPhone, egui's numbers through the core; not Windows): an array's several pictures share one zoom and view; Colour by Value and a number format on the grid, and colour on a table (`viewer.grid {colored, format}`, `viewer.table {colored}`). Compare's difference is not in it yet. Note 26 gains a capability table (section 11) and revised slices with tables as tables and the interchange formats (section 12), on eight assumptions made while the maintainer was away.
- Slices 2b, 4, 3 and 5 of note 26 (2026-11-24, overnight, branch `pr/views-s2b-5`): Compare's exact difference, made by the core and drawn by the Apple shells; a value typed `text/csv` shown as a table (one shared CSV reader, in the hostile-input sweep) and SVG text shown as a picture; column widths by dragging on the iPad, cell copy on Windows (built there, not launched); the plot laid out by the core as a display list (`chart.rs`) and painted by the Apple shells and by egui, whose own plot is retired. Launched on a scripted Mac copy and iPad simulators. Not done: Windows painters, egui's CSV and SVG-text views, SVG text longer than the excerpt.
- Slices 6 and 7 of note 26 (2026-11-25, overnight, branch `pr/views-s6`): a 1-D array shows as a line by index, reduced to the plot's width without losing a spike; a table's chart in the six kinds the maintainer named (histogram, line, stacked line, column, stacked column, treemap), chosen by `viewer.chart`, kept in the graph's `meta.charts` and undoable, laid out by the core and painted by the Apple shells, egui and Windows (Windows built, not launched). egui offers the CSV and SVG-text views; SVG text longer than its excerpt is drawn (both SDKs send the whole text). Hover and face previews are deferred.
- Slice 8 of note 26 (2026-11-25, overnight, branch `pr/views-s8`): time axes, shares of a stack (`normalize`), a chart's zoom and pan (`viewer.range`), a selection that belongs to the value so a table window and a chart window share it (`viewer.select`, `view.popout {another: true}`, the table's Selected Only), a readout of what is under a point, and a probe on Compare's difference. Apple shells and egui; Windows built, not launched. Open: hover and face previews wait on a decision (where the picture is made); the table follows a selection that arrives and marks whole rows (follow-up, same branch); on the Mac, after that scroll, the row above shows through the column header; egui's table shows no selection.
- Decided (2026-11-25): The maintainer, 2026-11-25, on the checklist page of 29 points (cards 1 to 15 are N1 to N15 here, 16 and 17 the release and merge points R1 and R2, 18 to 29 are V1 to V12 of note 26): "Decisions: 1A; 2A; 3A; 4A; 5A; 6A; 7A; 8A; 9A; 10A; 11A; 12A; 13A; 14A; 15A; 16A; 17A; 18A; 19A; 20A; 21A; 22A; 23A; 24A; 25A; 26A; 27A; 28A; 29B". The views points (18 to 28) stand as built; 29B: a long TEXT is not sent whole by the engine, the editor asks the engine for the rest (to build). Then, the same day: "Stop. Take the long text". So item 12 stands as built too (29A): the engine sends the whole of a long TEXT. Nothing to build.
- Face previews (2026-11-25, branch `pr/views-s10`; the maintainer: "Chart: a"): the array chart layout moved to a new crate, `plumcraft-chart`, with a rasteriser, and the Rust engine SDK sends the chart as the face's thumbnail for a 1-D array or a table array the core plots (no protocol change). The Python SDK does not yet. Apple and Windows have no wire or port hover of a value; egui's shows the same layout.
- Issue: none

### B148. Mac: the canvas picture from `ui.screenshot` comes out upside down and mirrored

- Reported: 2026-11-23, found by an agent while picturing an application built on the Mac shell; not reported by the maintainer.
- Report: `ui.screenshot` with no window named draws the canvas flipped top to bottom and mirrored. The real window draws correctly. The same is true of the public app's own picture from the curves work, so it predates that.
- Related, also unfixed: in a window that hosts an AppKit view (the mask editor), the offscreen picture draws the SwiftUI controls as a black band when the screen is locked.
- Cause: not known.
- Issue: none

### B144. `a_shared_remote_refuses_md5_uploads` failed once on the Linux test host, in a full run

- Reported: 2026-11-23, by the Linux gate on branch `feat/mask-editor` (the commit that moves the workspace to commonkit 0.3); not reported by the maintainer.
- Report: in one full nextest run (1,676 tests) `plumcraft-text::uploads a_shared_remote_refuses_md5_uploads` panicked at `crates/text/tests/uploads.rs:136` (the status of a five-byte upload was not 201). The same commit, the same host: the test alone passed 5 times of 5, and a second full run passed it.
- Not known: the reply it got. The gate's log keeps the failing line, not the assertion's message; the gate script on the host should keep it so the next sighting says more.
- Cause: not known. A timing fault under load is the controller's guess, not a finding: the test reads the reply until the engine closes the connection, right after starting the engine.
- Watch: record each sighting here with the reply.
- Issue: none
- Seen again: 2026-11-24, in the Linux gate on B151's branch (`ed7e6e3`, which changes no upload code). The second failure in a full run; still no cause.
- Seen a third time: 2026-11-24, in the Linux gate on B164's branch (`dadb1e0`, which changes no upload code). Two of the day's last four full runs.

### B143. The conformance case "the engine's run changed while it goes" failed once on the Windows test VM

- Reported: 2026-11-23, an agent (C# runner, ARM64)
- Report: step 26 expects `run.superseded` to change after a second `run.start`; it kept its value (5) because the first run had already finished between steps 22 and 24. It passed in three other runs the same session.
- Proposed fix: give the case a run that cannot finish by itself, as B123 did for the Mac person check with `text.gate`.
- Issue: none

### B140. iPhone, landscape: a table's pop-out spreads each record's names and values across the whole width

- Reported: 2026-11-23, an agent (B135's walk of the landscape presentations, the iPhone 17 Pro simulator at one step larger text)
- Report: turned, a value's pop-out is a full-screen sheet. That suits a picture, an array or a waveform, but a table's record view puts each field's name at the left edge and its value at the right, about 800 points apart, and "Sort by" and its menu likewise (`n4s5-ui-table.grid.png`, `n4s6-ui-table-compact.png` in `claude-debug/n4-decided-2/shots-walkphone*`). The same fault as B135 in a viewer: not changed with B135, since a viewer's width is the value's, not the controls'
- Open question: should a pop-out's lists (a table's records, a text's facts) keep to the readable width (`ReadableColumn.width`) while pictures keep the whole width?
- Issue: none

### B139. iPad, a 375-point window: the audio pop-out's bar runs past both edges

- Reported: 2026-11-23, an agent (B135's walk, a 375 by 655 window turned, one step larger text)
- Report: the audio viewer's bar (the kind menu, Play, Loop, the time, the volume) is wider than the sheet: "Audio" and "Loop" are cut at the left and the volume slider at the right (`n4s5-ui-audio.png` in `claude-debug/n4-decided-2/shots-narrow-run`). The iPhone in landscape shows it whole. A different fault from B135 (too wide for the window, not stretched)
### B138. iPhone: `testWindowsAndNames` doesn't get into the group; a dropped wire's palette opens instead

- Reported: 2026-11-23, the first run of every iPad UI test on the iPhone simulator (iPhone 17 Pro, one step larger text, portrait; branch `fix/b134-phone-add-node`, `conformance.sh gestures` with `PHONE=1`)
- Report: "timed out waiting for inside the group", then no `crumb.0`. The picture at that moment (`n4s3-ui-inside-group.png`) shows the node palette for a dropped wire ("Search kinds that take this…", "18 kinds take this wire"): the gesture meant to enter the group began on, or was taken as, a drag from a pin. 7 of the 9 tests passed on the phone and 1 was skipped (the test engine's)
- Not yet known: whether the test's target is too close to a pin at the phone's size (the test's own aim) or the gesture is read wrongly on a small screen (the app's). Not the same kind as B134: the palette it opened has its way out (Cancel, the swipe). In landscape on the same phone the test passes (B135's walk, 2026-11-23), so it is the portrait layout's
### B137. iPad: a path chosen for a graph the core holds no file for is stored relative to "/"

- Reported: 2026-11-23, found by the B133 conformance case on the iPad simulator (branch `fix/b133-path-browse`)
- Report: answering the path ask with `/plumcraft-b133-nowhere/chosen.txt` set the value `plumcraft-b133-nowhere/chosen.txt`. A chosen path is stored relative to the workflow's folder when it lies inside it (`paths::chosen_path`); with no workflow path the base is the process's current directory (`paths::base_folder`), which for an iOS app is `/`, so every absolute path lies "inside" it and loses its leading `/`. An in-process engine whose current directory is also `/` still finds the file, so nothing visibly breaks today; an engine elsewhere (HTTP, another process) would look in its own folder. The Mac and egui start in a real folder and aren't affected the same way
- Proposed: no current-directory fallback where the current directory is `/` (keep the path as chosen), or the shell names a base folder (the app's Documents) in the core's config
- Issue: none

### B132. `plumcraft-text::conformance mode2_process::off_the_run_path` failed once on the Linux test host

- Reported: 2026-11-22, the controller
- Report: in the Linux gate of branch `native/n4-prompts` the test panicked at `crates/text/tests/conformance.rs:205` (the `unwrap` of `plumcraft_conform::off_the_run_path`); the gate log kept no message beyond the panic's place. Run alone five times on the same host and commit straight after, it passed five times.
- Context: the branch's only Rust change is 12 lines in `crates/editor-core/src/invoke.rs`, which this test does not reach; the whole suite was running beside it. Not seen before.
- Open question: is it timing under load (the fixture uses a slow node measured in real seconds), like B39 and B40?
- Issue: none

### B126. On the iPad, a Save Result the engine asked about said "Couldn't save" and never offered the file the answer wrote

- Reported: 2026-11-22, found by reading while giving the prompts their labels (branch `native/n4-prompts`)
- Report: the iPad saves into a folder of its own and then offers the file to the export picker (`AskPresenter.exportWhenSaved`, B122's iPad half). When the engine asks the save's question (decisions 2A, 3A), the core's `save.last` turns to `ok: false` at the same moment it raises the `saveShape` prompt. The wait took that reply as the save's end, said "Couldn't save: …" under the prompt, and stopped, so the `.npy` (or PNG) the answer then wrote was never offered for export
- Fixed: 2026-11-22, branch `native/n4-prompts`: while the core's prompt is `saveShape` the wait keeps going and waits for the save the answer starts (none when cancelled). iPad UI test `testTheSavesQuestionAndARecipe` (the test engine attached over HTTP): Save as NumPy (.npy) brings the picker with `strip.npy`. Not run without the fix
- Issue: none

### B131. egui's `scrolling_in_past_the_picture_asks_for_detail_once` failed once: the panel's picture not yet reported

- Reported: 2026-11-22, the Rust gate on the Mac (branch `native/n4-prompts`, whose change to the core is the save question's first capital only)
- Report: `crates/ui/tests/kittest_viewer_tools.rs:257` unwrapped `watched.view.tools.pictures[0]` while it was still null ("invalid type: null, expected a sequence"): the test read the picture's place before the panel had drawn it. Passed when run again alone (2.3 s). Before this, the gate's runs today passed it
- Proposed: the test waits until the panel reports its picture, as its `until` does for the value, rather than unwrapping the first read
- Issue: none

### B128. On the iPad, Recipe drew "No recipe: the engine sent none for this value." over a value that had one

- Reported: 2026-11-22, seen in the iPad UI test's picture (branch `native/n4-prompts`; the test engine's Traced, attached over HTTP)
- Report: the viewer bar's Recipe (shared SwiftUI, `RecipeButton`) kept the recipe's text and the popover's `isPresented` in two separate states, set one after the other. In compact width the popover is a sheet, and the sheet drew the text it had before the reply: "No recipe". The app's own report (`recipe.text`) had the recipe, so a check that read only the report passed
- Fixed: 2026-11-22, branch `native/n4-prompts`: the popover is `popover(item:)`, its text travelling with its presentation. The iPad UI test `testTheSavesQuestionAndARecipe` now reads the drawn text ("traced", and no "No recipe"); the Mac person check reads the popover through accessibility as well as the app's report
- Issue: none

### B127. The save's question started in lower case ("this 4 by 1000 array is…")

- Reported: 2026-11-22, seen in the Mac person check's picture of the prompt (branch `native/n4-prompts`)
- Report: the core put the engine's reason, a clause starting in lower case, at the head of the prompt's text
- Fixed: 2026-11-22, branch `native/n4-prompts`: the core makes its first letter a capital (`Prompt::text`); test `invoke::tests::the_save_questions_text_is_a_sentence`. The SDKs' reason is unchanged (it is also the `config` error's text, inside other sentences)
- Issue: none

### B124. A default preferences file appeared in the maintainer's real Library, `Application Support/PlumCraft/prefs.json`

- Reported: 2026-11-22, the controller
- Report: a 57-byte file (`{"theme": null, "dirs": {}, "auto_engines": true}`, the core's preferences after `set_auto_engines(true)`) appeared at 15:29:31 in a folder that did not exist before, while agents ran the iPad simulator's UI tests and cargo tests on the Mac
- Looked into (branch `feat/sdk-save-2`): **not this workspace's Rust or Python tests.** That path is `plumcraft_editor_core::prefs::path_for("PlumCraft")` (moved only by `PLUMCRAFT_PREFS`), or a C ABI core given `dirs.prefs` there. No source in the workspace calls `path_for` with that name: the editor app's is `plumcraft-editor`, the C ABI uses only `dirs.prefs` or `PLUMCRAFT_EDITOR_PREFS`, the Mac app's folder is `Plummer` and the iPad's `plummer`. The whole Rust suite (1,639 tests) and the Python suite (247), each run with `HOME` at a fresh scratch folder (`claude-debug/sdk-save/home-probe.sh`, `home-probe-py.sh`), wrote no preferences there. The Rust run did write exported values under `Library/Caches/plumcraft/open/export` (`session::doc_cmds::default_root`), a second leak of the same kind. This agent's tests did not run at 15:29: its gate was compiling then, and its test stage ran from about 15:33. The Swift kit's tests, the iPad simulator's tests and the Windows kit's `dotnet test` were not run here: those are left as suspects. A Swift test that builds a `ShellConfig` with `appName: "PlumCraft"` and `dirs.prefs` under the real Library, without touching `testEnvironment` (which sets `PLUMCRAFT_EDITOR_PREFS`), would do it
- Fixed at source, for every process that is a test: `plumcraft_scratch::testing()` (a cargo-built test binary, or `COMMONKIT_TEST_RUN` set, as every scratch script and the Swift and C# test kits set it) puts `prefs::path_for` (and the trust file beside it) and `doc_cmds::default_root` under the process's scratch root. Tests: `prefs::tests::a_test_keeps_its_preferences_under_its_scratch_root`, `doc_cmds::tests::a_tests_copies_and_exports_stay_in_its_scratch_root`; the home probe after the fix finds nothing under the scratch home. Not covered: a C ABI core given `dirs.prefs` explicitly (the shell's choice)
- The real file was left untouched
- Found (2026-11-22, branch `native/n4-recipe`): **the writer was an app, not a test: an old trial build of the Mac app from when it was named PlumCraft** (`com.plumcraft.mac`, built 2026-11-21, kept in the main checkout's `claude-debug/native-n3/s6-try/n4s6/PlumCraft.app`). The system log has its process (`PlumCraft`, launched 15:28:26 by LaunchServices on demand and brought to the front, its parent launchd, no script's) doing file coordination and a recent-documents update at 15:29:31.81, the second the file appeared; that build kept its preferences in `Application Support/PlumCraft` (the app was renamed Plummer later; today's folder is `Plummer`). It was still running when found and was not signalled. The suspects on this side were cleared by running them: the Swift kit's tests with `HOME` and `CFFIXED_USER_HOME` at a scratch folder wrote no preferences (they did write `Library/Caches/plumcraft/open/export`, the export leak above, which `feat/sdk-save-2`'s `testing()` rule now keeps in scratch for them too, since their test environment sets `COMMONKIT_TEST_RUN`); the iPad simulator's conformance and UI tests, run again one at a time, left the file's time and inode as they were, and the iPad app keeps its folders in its container. Not a test leak, so no test fix; what would prevent a repeat: old trial builds left where LaunchServices registers them can be launched as a document's app, and run as the person's own app with the person's Library. Proposed: unregister and remove the trial builds under `claude-debug/*/s*-try` (the maintainer's decision; nothing was removed)
### B122. Save Result… did nothing from a node's menu or its face's footer in the native shells

- Reported: 2026-11-22
- Report: the Windows agent's reading (relayed by the controller): a menu item whose params carry `ask: true` (the node menu's Save Result…) runs with `command.run`, so `run.save` ran with no path and failed; the footer's Save Result… line is drawn but a tap on it is taken as a watch; no `run.save` anywhere under `native/`. Confirmed on the Mac by reading: `DocumentModel.activate` runs an item with params by `command.run`, and the core's tap on any footer item was `run.watch`
- Context: before the Save Result branch the core's `run.save` with `ask: true` worked only through `ui.invoke` (egui intercepted it itself). The maintainer's rule: a shell-side user action is a defect; the fix is the core's
- Fixed: 2026-11-22, branch `native/n4-save`, in the core, so no shell needs code: a tap on a footer item is decided by the item's kind (`geometry::FooterTap`): the `save` line's tap is `run.save {node, port, sheet, ask: true}` (Contact Sheet… on its right half when offered), invoked as the menu item is; `command.run run.save {…, ask: true}` is invoked too (replies `{invoked}`); a shell's own `ui.gesture` and such a `command.run` are its person's (the C ABI), so the shell shows the asks; over the control port they are a script's. `geometry.locate` takes `part: "save"`. Conformance: the save case now runs the menu item's way (`command.run`) and, where the shell measures its footer (`footer`: the Apple apps), taps the line and sees `file.save` asked. Windows can drop its shell-side dispatch (below, in the report)
- Issue: none

### B123. The person check failed one run in two under load: B87's banner, F36 cancel, F29 latest wins

- Reported: 2026-11-22
- Report: the controller's gate on the Save Result branch (screen locked, other heavy work on the Mac): three FAILs ("an unreadable manifest is said at launch (B87)" with `manifestProblems` present but no banner; "F36 cancel" "pick False disabled: no run in progress", the 6 s slow node already done; "F29 latest wins" "superseded 0 -> 0"); the next run, same code, passed
- Context: each step leaned on time. F36 and F29 waited at most 10 s for the slow node to show progress and then acted, so on a busy machine the node had finished (or not started) when Cancel or the second Run came. B87 compared the window's last banner, set only if the window saw the core's status line before a command cleared it, and any later banner replaced it
- Fixed: 2026-11-22, branch `native/n4-save`: the text engine has `text.gate` (passes its text once a file exists, stops on cancel; Rust and Python; a test each way), and the check's graph waits at it: the check removes the file, starts the run, waits on the face's state (`started`, `progress`) as long as the machine needs, cancels or starts the run over it (waiting on the core's `superseded` count), then puts the file back. F35's engine kill waits at the gate too. B87: the Mac reads the problems from the engine's state (`ui.inspect`'s `engine.manifestProblems`, which the core keeps) as its window comes up and says them in the banner; the status keeps every banner (`banners`), and the check looks for the problem among them
- Verified: the Mac gate five times in a row (screen unlocked; runs 2 and 4 with a busy loop per core beside them): 54 passed, 0 failed each time, B87, F29, F35 and F36 among them
- Issue: none

### B85. Plummer crashed twice in Swift's main-actor check from an AppKit callback while a script step drove a panel from inside a task

- Reported: 2026-11-21
- Report: found by the person check's first runs (branch `native/n4-person-gate`)
- Context: two crash reports (`Plummer-2026-11-21-210954.ips`, `…-211213.ips`, SIGSEGV): `swift_task_isCurrentExecutor` read a bad executor in `@objc CanvasView.isFlipped.getter` (during a menu's tracking) and in `@objc GraphWindowController.validateToolbarItem(_:)` (a toolbar validation timer), each right after the check's step had called the save panel's `ok(_:)` from inside the inbox's main-actor task. The likely cause: AppKit work that runs the run loop (a panel, a menu's tracking) inside a task's job lets other main-actor code run with that task's executor still current. A person's pick never runs inside a task, so this is probably the check's own defect; not shown to be impossible for the app (the `--then` steps' `menu:` and `context:` holds also run inside a task). Since the person check's steps run from the run loop itself (`PersonSteps.onRunLoop`), ten runs have not crashed.
- Issue: none

### B88. The Mac conformance case "the more-button opens the node's menu" failed once: the tap hit nothing

- Reported: 2026-11-21
- Report: the person gate branch's baseline run of `native/apple/PlumCraftMac/conformance.sh all` on the dev branch as it was (`d59d5ba`), before any change
- Context: step 3, `ui.gesture {kind: "tap", at: [284.126, 14.0]}` on the more-button's part the shell had measured: the core answered `hit: none` and cleared the selection, so no node menu came back (54 of 55). The next three Mac runs that day passed every case. Possibly the tap arriving before the app reported the header's parts (`geometry.parts`); not looked into.
- Issue: none

### B99. The suite's case "a kind chosen from a dropped wire's palette…" failed once against the Windows app: the palette's item had no port

- Reported: 2026-11-22
- Report: one of four ARM64 runs of the edit half on `native/n5-save`: step 14, `palette.items {query: "text"}` gave `text.literal` with no `port` (the palette for the dropped wire had become the plain palette); the next three runs passed it
- Context: not reproduced; the test engine was installed in that app's engines folder for the first time in these runs. Cause not found.
- Sighting (2026-11-23, branch `fix/b133-windows-browse`, the dev branch at `7fad6dc`): the same step 14 failed in two runs back to back against the ARM64 app, the C# runner's edit half (66 of 67) and then the Swift runner's from the Mac (59 of 60), each against a launch of its own. The x64 app passed it in the same round. The ARM64 app had had its warm-up launch, and was ready in 0.6 s. Cause still not found.
- Issue: none

### B96. The wire tiles' moiré at low zoom differs slightly from the per-wire drawing (observation, low priority)

- Reported: 2026-11-22
- Report: the N5 gate's pixel comparison (`claude-debug/native-n5/g-pixels.py`): at zoom 1 the tiles match the old drawing within 5 of 255; at zoom 0.37, 2.7% of pixels differ by more than 32, all inside dense bands of wires
- Context: the maintainer: "note for future". Not a defect to fix now (note 24's open items).
- Issue: none

### B92. A sound popped out by a double click on its pin started playing by itself, twice

- Reported: 2026-11-21
- Report: found setting up the sound for the maintainer to hear (`native/n5-gate`)
- Context: opening an Array to Audio value's window by a double click on its output pin, the window played at once twice out of six tries. The app's log says the Play button's `Click` handler ran (`sound: Play/Pause clicked`, then the core's `viewer.audio` state playing, seq 1), with no click of the script's on the button (the pointer was on the pin, outside the new window) and no key sent. With handlers added that log every pointer press, release and key on Play, three more tries didn't play. The core never starts a sound on its own (`viewer_state.rs`: `playing` starts false). Whether the Apple apps or egui do the same was not checked. Cause not found.
- Issue: none

### B55. On the Windows test VM a Plummer process stuck where nothing could end it, and the console session stopped starting programs

- Reported: 2026-11-21
- Report: found while taking N5 slice 4's screenshots (`native/n5-panels`); recorded under the standing "record bugs"
- Context: a screenshot run launched the app (`edit-number`, 15:17) through a one-shot Interactive scheduled task, as every run does. It never wrote its first window to its status file. From then on no program started in the console session: every later scheduled task's process (cmd, PowerShell, the screenshot and input injectors) stayed at its start, and the app's process and the console host and terminal processes launched with it couldn't be ended (`Stop-Process` and `taskkill` reported success or access denied, and the processes stayed). Windows logged a `LiveKernelEvent` (code `1cc`) at 15:31 and an application hang in another app. A program started from the SSH session (session 0) still runs. The VM was not restarted (the brief: never restart it); the stuck processes are still there. Whether the app caused it, or the VM's display driver did and the app was the first to meet it, is not known.
- Looked at after the maintainer restarted the VM: Windows logged `LiveKernelEvent` `1cc` (a kernel resource held too long, a live dump in `C:\WINDOWS\LiveKernelReports\ResourceTimeout-20261121-1520.dmp`) at 15:20, three minutes after the stuck launch, and again at 15:30 and 15:31 with hang reports for two system apps (the Store, the AAD broker). There was no crash or hang report for the stuck Plummer itself, no WerFault and no debugger attached, and the app never wrote its first window to its status file, so it stuck before any window, sheet or dialog of its own was shown, not on a modal sheet and not in a crashed composition thread. Every launch then went through `cmd /c` in a console that Windows handed to Windows Terminal (its default terminal), and Windows Terminal, OpenConsole and conhost started with that launch were among the processes nothing could end. Since the restart every scripted launch runs under `conhost.exe --headless`, so no terminal is started; it did not recur in the seven launches for the suite after the restart (four on ARM64, two on x64, one from the Mac's runner); later the VM stopped answering altogether (no ping, no SSH), and was left so. The dump was not opened (no kernel debugger on the VM). On `native/n5-gate` (2026-11-21, evening, after the maintainer restarted the VM with 10 GB): the person check (`native/windows/person-check.ps1`) ran twice to its end, the app quitting by File › Quit the second time; a minute later SSH accepted connections but sent nothing, ping still answered, and some minutes after that the VM stopped answering ping too. Whether this is B55 again is not known: nothing could be read from the VM. Twice more since: on 2026-11-21 at 22:36 (Mac clock), a minute after scripted double clicks on a pin and as `dotnet test` started; and on 2026-11-22 at 09:35:29, with the VM's emulated TPM turned off by the maintainer, about a minute into the person check, after cargo, the app build and four `dotnet test` runs had all passed without a lost ping. So the TPM is not the cause, or not the only cause. Each of the three came during or soon after a script that drove the app through UI Automation and SendInput; many such runs did not freeze it.
### B39. `bytes_held_elsewhere_are_shared_not_read_again` failed once on the Linux test host

- Reported: 2026-11-20
- Report: recorded by the controller under the maintainer's "record bugs"
- Context: `plumcraft-media` `service::tests`, `crates/media/src/service.rs:1133`, "let go of everywhere: read again": after the test dropped its two handles on the bytes, the service still answered with them. Failed once in the full run for N3 slice 2; passed 300 of 300 alone. Not looked into; whether a worker can still hold the bytes at that moment is the first thing to check.
- Issue: none

### B40. `scrolling_in_past_the_picture_asks_for_detail_once` failed once on the Mac under the full parallel run

- Reported: 2026-11-20
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: `plumcraft-ui::kittest_viewer_tools`. Failed once in the full parallel nextest run for N4 slice 2 on the Mac; passed 20 of 20 alone. The message was not captured. Not looked into.
- Seen again (a neighbour): 2026-11-21, `kittest_viewer_tools::a_click_pins_the_probe_and_the_command_does_the_same` failed once in a full parallel run on the Mac (the person gate branch's core work) and passed alone; the message was not kept.
- Issue: none

### B54. The Windows app ended once with a stowed exception during the conformance suite's read half

- Reported: 2026-11-21
- Report: found by the gate of N5 slice 4 (`native/n5-panels`); recorded under the standing "record bugs"
- Context: the first run of the read half against slice 4's build on the Windows test VM: the app process ended (exit `0xC000027B`, a stowed exception; Windows Error Reporting names `CoreMessagingXP.dll`, `combase.dll` and `E_INVALIDARG`) around the case "a watched picture arrives as picture", while pop-out windows the core opened for the earlier cases were being closed by `graph.new`. Nothing reached the app's own handler or log. Pop-out and panel windows now drop what comes for them after they close (a value read, a viewer's page, a report), every viewer's update catches and logs its own faults, and `PLUMMER_TRACE` logs every exception as it is thrown. The run of both halves after these changes passed, and about forty scripted launches for screenshots ended normally; the x64 run and repeated runs were not taken (B55); the cause was not found, so this stays open. After the VM's restart, with every exception logged as thrown (`PLUMMER_TRACE`): the suite four times on the final ARM64 build, twice on x64 and once from the Mac's runner, every one to the end with the app stopped by its PID; no stowed exception and no app exit. Not reproduced.
- Issue: none

### B52. `CoreFlow.ManyRequestsInFlightAndWakesCoalesce` failed once on the Windows test machine: "busy: the document's engine is still connecting"

- Reported: 2026-11-21
- Report: found by the gate of N5 slice 3 (`native/n5-editing`); recorded under the standing "record bugs"
- Context: `native/windows/tests/PlumCraft.Kit.Tests/CoreTests.cs:149`, the Windows kit's test that sends 500 requests at once to a core just opened. In one of four `dotnet test` runs on the Windows 11 ARM64 test machine (40 of 41 passed) the core refused one with "busy: the document's engine is still connecting": the C ABI core holds at most 256 requests for a document whose engine hasn't connected yet (`MAX_HELD`, `crates/ffi/src/core.rs`), and the test doesn't wait for the engine before sending them. The run before and the two after passed 41 of 41. Not changed: the test or the limit, whichever is wrong, is for the maintainer.
- Decided (the maintainer): "Make the test wait". On `native/n5-gate` the test sends `scene.get` first, which the core holds until the document's engine connects (`ping` is the core's own and isn't held), and only then the 500 requests. On 2026-11-22 it passed in three full `dotnet test` runs on the Windows test VM (TPM off); left open for the maintainer to close.
- Issue: none

### B119. On an iPad with Windowed Apps, in a small window, the bar lost Run and the More menu to the system's overflow

- Reported: 2026-11-22
- Report: the controller's two device runs of the iPad UI tests (the maintainer present). Run B, on PR #163: `testCompactWindowsByATap` and `testNoKeyboardWithNothingEdited` failed with no `tool.more` button; the bar's elements were Documents, Add Node, the title menu and the system's own `OverflowBarButtonItem` "More". Every status printed at a time-out in both runs said `"size": (375, 655.5)`. The maintainer: "It's on Windowed apps" (Settings, Multitasking & Gestures)
- Context: with Windowed Apps a window can be any size and iPadOS keeps the size it last had, across launches and test runs. In a 375-point window the window controls sit beside Documents, and Documents, Add Node, the title, Run and More don't fit: the system folds Run and More into its own overflow menu (More inside "More"), so a person reaches Run and the core's menus two menus deep, and a test finds neither
- Reproduced (branch `native/n4-window`, the iPad Pro 11-inch (M5) simulator with Windowed Apps chosen in its Settings, a 375 by 655 window): the same bar, picture for picture
- Fixed: 2026-11-22, branch `native/n4-window`: a narrow window (compact and under 440 points) keeps Documents, the title, Run and More on the bar, and Add Node moves into the More menu (`GraphViewController.narrow`; the bar is laid out again when the width crosses it). `testCompactWindowsByATap` and `testNoKeyboardWithNothingEdited` pass at 375 by 655
- Issue: none

### B120. On the iPad, in a window that isn't the whole screen, the field being edited stayed under the keyboard

- Reported: 2026-11-22
- Report: found reproducing B119 on the simulator at 375 by 655 (Windowed Apps): `testInlineEdits` timed out on "the keyboard up, the field above it"
- Context: the keyboard's frame comes in the screen's coordinates; the row editor converted it as if it were the window's (`FieldEditorHost.keyboard`), right only when the window is the screen. In a window placed elsewhere on the screen the keyboard's top was computed off by the window's offset, so the canvas wasn't lifted enough (or was lifted when it needn't be)
- Fixed: 2026-11-22, branch `native/n4-window`: the frame is converted from the screen's coordinate space (`canvasView.convert(end, from: win.screen.coordinateSpace)`). `testInlineEdits` passes at 375 by 655
- Issue: none

### B121. The iPad UI tests assumed a whole-screen window

- Reported: 2026-11-22
- Report: the device runs above (B112 to B114 at 375 by 655.5) and the same tests on the simulator at 375 by 655
- Context: test faults, not the app's. The tests never set the window's size, and on a device with Windowed Apps the size is whatever the window last had (run A all at 375 by 655.5; yesterday's run started at 1,067 by 655.5 and was 375 by 655.5 after `testTouchGestures` rotated the device; run B 375 by 655.5 until the same test's rotations, then the whole screen; on the simulator a window zoomed to the whole screen went back to its earlier 375 by 655 when that test turned the device). They tapped points that were off the window or under the bar (the table's pin above the window's top, B112; the group's title, B114), dismissed menus and the palette by tapping the app's own left edge (the palette is a sheet from below in a narrow window), and tapped toolbar buttons (Fit, Select) that a narrow bar keeps in its More menu (B113); the warned-engine test scrolled the Engine menu by swiping an item that had scrolled away (it now swipes the lowest item in reach, and asks for the whole screen whatever `PLUMCRAFT_WINDOW` says: in a 375-point window the menu is short, the system's keyboard covers its foot, and a scroll of it was taken as a press often enough to be no test); `testEveryViewerByATap` opens each viewer's node centred, so it works at any size
- Fixed: 2026-11-22, branch `native/n4-window`: each launch asks for its window (`--window-size`, the whole screen unless `PLUMCRAFT_WINDOW` names a size): the whole screen through the system's window controls (Zoom) when the window isn't already, a smaller size by the app pinning its scene's size and the test dragging the window's corner to it, which is how iPadOS lets a window shrink; every test says its window once ready (`WINDOW …`: size, size classes, activation state, orientation, the size asked for) and every time-out says it too; points are panned into reach first, as a finger would (`reveal`); toolbar buttons come from the bar or the More menu (`toolbar`); taps outside a menu or the palette go to the canvas's top left below the bar
- Issue: none

### B111. On the iPad, after a number typed into a face's field, the app never reports idle (XCTest waited 60 s on every step)

- Reported: 2026-11-21
- Report: the controller's device run of the iPad UI tests (the maintainer present; built from the main checkout, `build-device/test`): `testInlineEdits` passed but took 863 s; "App animations complete notification not received" 14 times in the run
- Context: the log shows the first field (text) typed into and Done tapped normally; then `7` typed into the `count` row's number field (the numbers-and-punctuation keyboard), and from the tap on `row-editor-done` that follows the app never reported idle again: every later step waited 60 s first. So something starts animating or redrawing with the number field's editor and doesn't stop after it closes; a battery defect on a device too.
- Looked into (branch `native/n4-decided-2`, no device use): **not reproduced** on an iPad Pro 11-inch (M5) simulator booted headless (the software number keyboard shows, as on the device), in portrait, with the status read from the file and again from the accessibility element as the device read it: `testInlineEdits` passed in 21 s and 22 s, no idle wait. What runs without end in the iPad's canvas: only a running face's wash pulse (`LayerCanvas`, `repeatCount = .infinity`, while a face shows running); the pan momentum's display link stops itself; the wire tiles have no timer; the keyboard's lift (`FieldEditorHost.keepAboveKeyboard`) animates the viewport once per keyboard frame change. Candidates on a device: a face left showing running (its pulse), or the keyboard lift and the keyboard's frame notifications feeding each other at the device's keyboard height. Needs the device: re-run with the app reporting what animates (a pulse, the momentum link, a viewport animation) in its status.
- Looked into again (2026-11-22, branch `native/n4-engine-half`, no device use): both Apple apps now report what is animating in their status (`animating`: a running face's pulse, the run colours' fade, an animated pan or zoom, the reveal, the pan's momentum, the keyboard lift; empty when idle), and the UI tests print it on every "timed out waiting". `testInlineEdits` now asserts it empties within 8 s of the number field's Done. On the iPad Pro 11-inch (M5) simulator, booted headless so the software keyboard shows (it did: the numbers keyboard, as on the device), with the status read from the file and then from the accessibility element as the device reads it, the test passed every time (four runs) and `animating` emptied: **still not reproduced.** The device re-run now says what was animating if it happens again.
- Read again with B118 (2026-11-22, branch `native/n4-keyboard`, the device log only): B118 does not explain it. The first idle wait starts after `7` is typed into the number field, while the field is still open: the typing's own idle wait ended at 12.02 s, and the next one, XCTest's wait before the tap on `row-editor-done` 0 s later, never ended (no menu open). So the app went from idle to never idle within a fraction of a second after the `7` reached the field, before Done; all 14 waits of 60 s are in `testInlineEdits`, and the next test (a new launch) has none, so whatever it was ended with the app. The log has no keyboard frames and no first responder (the app didn't report them then). So the candidates stay what they were: the number field's keyboard and the keyboard lift feeding each other, or a face left running. Both now show in the status: `animating` (`keyboard-lift`, `pulse`), and `keyboard` with `firstResponder`. B112 to B114 are not B118 either: their element lists show no keyboard, and B113's missing Select button is the bar's overflow (the More button, `OverflowBarButtonItem`, was there with Documents, Add Node and the title menu), a width matter, not a covered bar
- The device runs of 2026-11-22 (the controller, the maintainer present): run A (the build before PR #163) had it again, 14 idle waits of 60 s from just after `7` was typed, with `animating` empty throughout; run B (#163, 20 minutes later) had none, `B111: animating 8 s after the number's Done: []`, in 21 s. Both ran in the same 375-point free-size window (the iPad is on Windowed Apps, B119), with the number keyboard up. So not the window's size alone, and not one of the app's own animations (`animating` stayed empty while XCTest waited). On the simulator in a 375 by 655 window with Windowed Apps, the test passes in 21 s, in portrait and three times turned landscape as the iPad was (20 to 26 s, no idle wait). Found on the way: B120 (the keyboard's frame read in the window's space, wrong for a window not at the screen's origin), fixed; it would misplace the lift, not keep the app from idle. Still open; the next run's every wait says the window (`WINDOW`), the keyboard and the first responder
- Time-boxed again (2026-11-22, branch `native/n4-prompts`, ten minutes by the clock from 20:33, the two device runs' logs at `a4bfb5e` and the code only, as the maintainer ordered): **not solved; it stays open.** What the logs say. Full screen (run 1): the first wait that never ends is the one right after `7` is typed into the number field (the typing's own idle wait ended at 9.33 s; the next, before the tap on `row-editor-done`, ran out at 69.52 s), and from then every action's pre- and post-event wait ran its 60 s until the app was terminated: 14 in `testInlineEdits` (859 s). In `testNoKeyboardWithNothingEdited` it started the same way, after text was typed into a field (`no keyboard`, 17.30 s), and ran on through Done (3 waits, 202 s). At 375 by 655 (run 2) the same, 18 waits. The app's own `animating` was `[]` at the one dump taken (8 s after the number's Done); `keyboard` and `firstResponder` were not dumped at a stall, since no test wait timed out. So: it starts when the software keyboard types into a field of the canvas's own (a `KeyedTextField` over the canvas), on the device only, and nothing the app tracks (the pulse, fades, animated pans, the reveal, momentum, the keyboard lift) is running. Not checked: the field's caret and the keyboard's own layers, display links outside `Activity` (`CanvasBench`'s is bench-only; the momentum link is tracked). What was done instead (the maintainer: "go to skipping the idle wait"): the UI tests no longer wait for XCTest's idle (below, in the report and `GestureTests.swift`), and the app's status now lists, when asked, every layer animation running in its windows and its display links, so the next device run carries the evidence at no cost
- The probe's first device run (2026-11-22, the merged build `4366fe9`; 30 minutes on it after, branch `native/n4-b111`, no device). With the number typed, the device listed the text cursor's blink (`_uitcvba` on a `UIView`'s layer, 1 s, repeating without end), the keyboard's arrival (0.3833 s: `UIKeyboardItemContainerView`, `UIKBInputBackdropView`, `TUIInputAssistantHostView`, `TUISystemInputAssistantView`, `_UIKBCompatInputView` bounds and position; the Liquid Glass layers' `effect.*` and opacity on `UISDFView`) and the glass's `match-*` animations (`UISDFElementView`, no duration). 8 s after Done: no cursor blink, but the keyboard's going still attached (`position-2` on `UIKeyboardItemContainerView` and `TUIInputAssistantHostView`, `cornerRadii` on `UIKBBackdropView` and `_GlassGroupView`, the accessory bar's own `UIToolbar` `position`, an `opacity` of 0.47 s on a `UIImageView`, `match-*` on `GlowContainerView`), all of 0.38 to 0.47 s. **The same probe on the simulator (iPadOS 26.5, `testInlineEdits` with the status read from the element):** with the number typed, the cursor's blink and the glass's `match-*` animations, and no keyboard animation; 8 s after Done, only the `match-*`. So: (1) the cursor's blink is not the cause: the simulator has it too and XCTest reaches idle there, and it is gone after Done on both; ours is the system's cursor in a plain `UITextField` subclass, resigned and removed from the view tree on Done (`FieldEditorHost.remove`: `resignFirstResponder`, `removeFromSuperview`), so no hidden first responder is kept. (2) What the device alone has is the keyboard's own animations, its arrival and its going, still attached long after their 0.38 s: finished-but-kept or never run is not known (the probe now says, per animation, `age`, `ended`, `kept` for not removed on completion, `fill`, and each window's class, hidden state and alpha). XCTest waiting for those to complete matches every stall seen: from the keyboard's arrival with a typed field, through Done, until the app ends. (3) Not shown to be ours: the animations belong to UIKit's keyboard on iPadOS 27 (the device's build `24A94232`); the one view of ours among them is the field's input accessory bar (`UIToolbar`), which the keyboard animates with it. **The next four-minute device run should check:** the same eight tests as tonight, reading the new fields: whether the leftover animations are `ended` and `kept` (UIKit's keyboard keeping finished animations: not ours to fix; skipping the idle wait is then the answer, as now) or not `ended` after seconds (an animation that never runs, as in a hidden window, which the window list will show); and, if a minute can be spared, `testInlineEdits` alone with `PLUMCRAFT_IDLE_WAIT=on`, to see whether the idle waits still stall on this build
- Issue: none

### B112. On the iPad, the table viewer's output pin double-tapped did not pop it out (`testEveryViewerByATap`)

- Reported: 2026-11-21
- Report: the controller's device run: "timed out waiting for tab popped out in table.grid", "tab: the pop-outs were []", then no `popout-done`
- Looked into: **not reproduced** on the simulator (status by file and by element): the test passed in 28 s and 30 s, the table first. The test predates today's pop-out and grid-window changes, but it passes against them here; the device ran the main checkout's build, not this branch. The first viewer is the one a double tap right after launch hits: on a device the first double tap may land before the faces' parts are measured (as B88 on the Mac), or the system takes it as two taps (the test retries once after 8 s). Needs a device re-run.
- Explained (2026-11-22, branch `native/n4-window`): the device's window was 375 by 655.5 (Windowed Apps, B119; run B said so at the time-out too), and centred on the sound node the table's pin was above the window's top: the double tap landed outside the canvas. A test fault (B121): the test now pans each pin into reach first, as a finger would, and passes at 375 by 655 on the simulator. Not an app fault: the pop-out opens by a double tap at that size
- Issue: none

### B113. On the iPad, `testTouchGestures` failed: the pinch did not zoom, the palette was not dismissed, the marquee selected nothing, no Select button

- Reported: 2026-11-21
- Report: the controller's device run: "timed out waiting for zoomed in by the pinch" (zoom stayed 1), "the palette dismissed", "the marquee selected", and `GestureTests.swift:122` and `:226` (no "Select" button)
- Looked into: **not reproduced** on the simulator: passed in 39 s and 40 s. The later failures follow the first (the zoom never changed, so the points the test aims at were elsewhere). The Select button is the toolbar's Select mode (`canvas.selectMode`, N4 slice 3); a device window narrower than regular width puts it in the More menu, where the test doesn't look. Needs a device re-run, with the window's width class and the toolbar's items in the failure's attachments.
- Explained (2026-11-22, branch `native/n4-window`): the window was 375 by 655.5 (B119). The test dismissed the context menu by tapping the app's own left edge (inside a face or off the canvas in a small window), so the menu stayed and the pinch, the palette and the marquee that followed went astray; Fit and Select are in the More menu of a narrow bar. Test faults (B121): taps outside go to the canvas's top left below the bar, the marquee spans what a finger reaches, toolbar buttons come from the bar or More, and in a free-size window the turns check the graph is still shown rather than the window's shape. Run B passed it (the whole screen after the turns). Passes at 375 by 655 on the simulator; the turns there leave a free-size window at 375 by 655
- Issue: none

### B114. On the iPad, `testWindowsAndNames` failed: not inside the group, no `crumb.0` button

- Reported: 2026-11-21
- Report: the controller's device run: "timed out waiting for inside the group", no `crumb.0` button (`GestureTests.swift:397`)
- Looked into: **not reproduced** on the simulator: passed in 20 s both times (B60's step, Show on Face after a hidden value is set, passed too). Entering the group is a double tap on its face; the breadcrumb appears only inside. Needs a device re-run.
- Explained (2026-11-22, branch `native/n4-window`): the window was 375 by 655.5 (B119) and the group's title was off the window, so the double tap missed it. A test fault (B121): points are panned into reach first. Run B passed it at the whole screen; it passes at 375 by 655 on the simulator
- Issue: none

## Closed, not reproduced

### B60. `testWindowsAndNames` fails on the iPad: Show on Face not found after editing a hidden value

- Reported: 2026-11-21
- Report: the controller, from the iPad UI test run on the maintainer's iPad (portrait, 834 by 1210 points, the dev branch at `50b8aec`)
- Context: `native/apple/PlumCraftPad/UITests/GestureTests.swift:359`. Four of five UI tests passed on the device (`testCompactWindowsByATap`, `testEveryViewerByATap`, `testInlineEdits`, `testTouchGestures`); `testWindowsAndNames` failed. After the parameters popover opened on node `w`, the field `edit.count` was found, tapped and typed into (`5\n`; the status then said "Set Value"), and `app.buttons["face.count"].tap()` failed with "Failed to get matching snapshot: No matches found for Elements matching predicate '"face.count" IN identifiers'". The test passes in the simulator.
- Looked into (2026-11-21, branch `native/n4-wire-hitches`): **not reproduced, cause not found.** In an iPad Pro 11-inch (M4) simulator in portrait, booted headless so the software keyboard shows as on the iPad, with the status read from the accessibility element as on a device, the test passed every time: at the default text size, at the three largest standard sizes and at Accessibility Large. The popover opens above `w`, in the space between the toolbar and the node, and shows about three rows; while the count field is edited UIKit keeps its row in view, and after Return and the list's reload the row and its Show on Face button are still there. Nothing in the popover's code (`PlumCraftPad/App/Parameters.swift`) dismisses it on Return or changes on a device. Ruled out in the simulator: the software keyboard, a larger text size, the status element. Not ruled out: timing on the device (the test tapped `face.count` the moment the status said "Set Value", while the keyboard was going and the list reloading; every other step of the test waits for its element), and anything the device does that the simulator doesn't. **The test now waits for the button (10 s), looks for it in the list both ways if it isn't in view, and attaches a screenshot (`b60-face-count-missing`) and the app's element tree (`b60-element-tree`) when it still can't find it,** with screenshots while the keyboard is up (`n4s3-ui-count-typing`) and once the value is set (`n4s3-ui-count-set`). No app change: no app defect was shown. To confirm: `native/apple/PlumCraftPad/device.sh ui` on the iPad; if `testWindowsAndNames` passes, the cause was the tap's timing; if it fails, the two attachments show what the popover held.
- The device run after it (2026-11-21, the controller, the maintainer present; the main checkout's build): `testWindowsAndNames` failed again, but earlier, at entering the group (B114), before the step B60 is about, so this run says nothing of B60. Five tests: 2 passed (`testCompactWindowsByATap`, `testInlineEdits`, the latter in 863 s, B111), 3 failed (`testEveryViewerByATap` B112, `testTouchGestures` B113, `testWindowsAndNames` B114), 11 failures, 1,185 s. On an iPad Pro 11-inch (M5) simulator in portrait (branch `native/n4-decided-2`) all five passed twice, the status read from the file and from the accessibility element. Still open: a device re-run decides it.
- The device runs of 2026-11-22: run A failed earlier again (the group, B114, in a 375-point window); run B passed `testWindowsAndNames` whole, B60's step included, at the whole screen (after `testTouchGestures`' turns made the free-size window whole, B121). So B60's step passes on the device. Proposed: close as test timing, as decided (decision 16), once a run with the window set (every launch asks for the whole screen now) passes it again
- Decided: 2026-11-22, the maintainer (decision 10A): close it once a device run with the window set passes `testWindowsAndNames`; open until then
- Closed: 2026-11-22, as decided (decision 10A): the device run of the merged build `4366fe9` (the controller, the maintainer present; the window set, every launch asking for the whole screen: `WINDOW … fullScreen 1, asked full`) passed `testWindowsAndNames` whole in 29.9 s, B60's step included
- Issue: none

Closed without a fix: the maintainer decided they need no more work unless they are seen again.

### B118. On the iPad simulator the software keyboard was up after a launch with nothing being edited

- Reported: 2026-11-22
- Report: seen in a picture taken by the new UI test `testAWarnedEngineIsMarkedInTheMenu` (branch `native/n4-engine-half`), the first test of its run, right after launch and with the More menu and its Engine submenu open: the full software keyboard was on screen though no field was open
- Context: the canvas's controller becomes first responder at launch (`GraphViewController`, for the hardware keyboard's commands) but takes no text; no other test's picture shows the keyboard without a field. Seen in each run of that test whose picture or element list I read (five), a test which launches with no `--then` steps (the other tests' first step may put it away); the test's tap on the keyboard's Hide key didn't hide it. Not looked into further. On a device with no hardware keyboard a keyboard over half the screen is what a person would see. Worth checking with B111 (the device never idle once a field's keyboard had been up)
- Looked into (2026-11-22, branch `native/n4-keyboard`, the iPad Pro 11-inch (M5) simulator booted here, software keyboard on, no device use): **the system's, not the app's; the report was wrong about when.** The iPad app now writes the on-screen keyboard (`keyboard`: shown, its frame) and what holds the keyboard's focus (`firstResponder`) to its status whenever the keyboard shows, hides or moves (`KeyboardWatch`, debug and release). After the launch there is no keyboard (status and accessibility agree, in ten launches: alone, after the iPad conformance, both samples). It comes up when a menu opens (the bar's More menu, the title menu) and goes when the menu closes, with the graph's controller still first responder and no text field or search field anywhere. The stock Files app's More menu brings the same keyboard in the same simulator. Taking away the controller's first responder, its key commands, or its editing-interaction setting changes nothing. The earlier pictures were taken with the menu open, which is why the keyboard looked present "after launch". What is left is a test matter: the warned-engine test scrolls the menu from an item the finger can reach, and its Hide-keyboard tap is gone
- Test: `testNoKeyboardWithNothingEdited` (the iPad UI tests) asserts no keyboard after the launch, after a menu is opened and closed, and after a text field's Done, and that one is up while the field is edited
- Closed: 2026-11-22, the maintainer (decision 11A): not the app's
- Issue: none

### B46. A Cancel Run was not honoured once in the egui editor (unconfirmed)

- Reported: 2026-11-21
- Report: found by the egui audit; recorded under the standing "record bugs"
- Context: a 5 s Slow node finished `done` at 5,008 ms after Cancel Run; seven later attempts all cancelled within 2.4 s, and the first run's log was overwritten. Seen once, on the Linux test host. `claude-debug/egui-audit.md`, defect D9.
- Tried (2026-11-21, egui parity, on the Mac): 510 cancels of a 1.5 s Slow node at delays from 0 to 1,000 ms after `run.start`, nothing cached: 150 in the egui editor (the command and the menu's invoke alternately), 120 with a run already going when `run.start` superseded it (Auto-Run's or another `run.start`), 90 through egui's control dispatcher (`ui.invoke run.cancel`, `ui.waitRun`), and 150 through the C ABI core (`command.run` and `ui.invoke {by: person}`). Every one ended `cancelled`. In the audit's sequence the cancel came after 40 frames, the first GPU picture of that process on a software renderer, `ui.inspect` and `menu.get`; if those took over 5 s, the node had finished before Cancel was sent, which would explain it. Scratch drivers: `claude-debug/egui-parity/b46/`.
- Closed: 2026-11-21, not reproduced, by the maintainer ("Close", asked whether to close it or rerun the audit's sequence on the Linux test host first). 510 attempts, every one cancelled; the likely explanation is the one above: the audit's Cancel came after 40 frames, the process's first GPU picture on a software renderer, `ui.inspect` and `menu.get`, and if those took over 5 s the node had already finished before Cancel was sent.
- Issue: none

## Fixed

### B161. iPhone: the pop-out's bar overflows since the View menu was added

- Reported: 2026-11-24 (found by an agent in its own earlier work, not by the maintainer)
- Report: the View menu added by B154 slice 1 makes the iPhone's compact viewer bar too wide: the Pictures picker is cut at the left and the icons at the right. Seen in an iPhone simulator picture taken while checking B159.
- Issue: none
- Fixed: 2026-11-24, branch `pr/views-s2`: the viewer bar takes the widest of three arrangements that fits (one row, two, or three); on the iPhone it is two rows, all inside the window. UI test `testTheViewerBarFitsANarrowWindow`, passed on an iPhone simulator. Not on a device.

### B166. Mac: Open Recent never lists recent graphs

- Reported: 2026-11-24
- Report: "open recent never has a list of recent graphs"
- Context: the maintainer's hand use of an application built on the Mac shell from the dev branch at `3403749`, after opening and saving several graphs today (`.plum` and `.json`).
- Answer (the maintainer, 2026-11-24): asked whether the menu is empty or greyed, and whether it ever listed anything: "empty list. Never showed anything".
- Issue: none
- Cause (reproduced in Plummer itself): Open Recent was AppKit's menu. AppKit did note every graph, but the Mac rebuilds its menu bar when the front document or the menu's structure changes, and AppKit fills only the Open Recent menu it adopted first. The core had no list at all, so it was a shell-side action, which the command rule counts as a defect.
- Fixed: 2026-11-24, branch `fix/b165-b166-files`: the list, the menu and its items are the core's. `Prefs.recent` (newest first, once each, 10 kept, in the application's own preferences); a successful open, save, save as, rename or move notes the file; File › Open Recent in the core's menu model, a missing file disabled with why, then Clear Menu; commands `graph.recent`, `graph.openRecent {path}` (only a path in the list) and `graph.clearRecent`. egui, the iPad and Windows get the menu from the model. `docs/shell-interface.md` 12.24. Two test files that fail without the recording, a shared conformance case. Launched on the Mac: the list read back newest first, a deleted file disabled, kept across a relaunch, emptied by Clear. Not checked: picking an item by hand; the Mac with every window closed; iPad, Windows and egui launches; the application built on the shell.

### B165. Mac: Insert Graph of a component graph inserts nothing

- Reported: 2026-11-24
- Report: "insert graph (components/condition) inserts nothong"
- Context: the maintainer's hand use of an application built on the Mac shell from the dev branch at `3403749`. The graph inserted is a component graph of the research software's collection (a "condition" component, in a `components` folder).
- Answer (the maintainer, 2026-11-24): "inserted condition.plum"; then "Fix b165". The component exists in both forms, `.json` and `.plum`.
- Issue: none
- What a person saw (an agent, Plummer built before the fix, the menu's command run through the control port): the graph does not change. The refusal shows once as a banner; a second try at the same file shows nothing, since the Mac shows an error only when its text differs from the last (B167).
- Cause (proved by a test and a launch): `graph.insert` checked the file against the connected engines' node types only. Where engines start as a graph comes to use them, a component whose nodes belong to an engine not yet started was refused, before the step that starts newly needed engines could run. `graph.open` keeps kinds it does not know yet and then starts their engines. Not the extension (`.json` fails the same way), not the group, not the Mac's panel: the controller's first suspicion, the `.plum` type, was wrong.
- Fixed: 2026-11-24, branch `fix/b165-b166-files`: the file is checked and inserted against the catalogue plus the node types of engines the host can start (`crates/session/src/insert_cmds.rs`, `Session::edit_against`), and the needed engine then starts. A kind no engine offers is still refused, by name. A test with a public component, as `.plum` and `.json`, fails without the fix. Launched in Plummer: the group, its promoted port, its output and its wires arrive and the array engine starts. Not checked: the application with the report's own file; iPad and Windows (the same core).

### B162. Mac: after wiring a range driving node into a style graph, the graph no longer runs at all

- Reported: 2026-11-24
- Report: "After wiring styles/cheap-and-cheerful-2bug.plum to have a range driving node, the graph no longer runs at all." Then: "fix B162 (deleting the range let's it run)".
- Context: the maintainer's hand use of an application built on the Mac shell from the dev branch at `98f1883`. The graph is a style graph of the research software: a range driver of the text engine wired to a setting of a node of the application's own engine, and that setting's name holds a dot (a sub-step's setting, of the form `edges.threshold`).
- What a person saw (an agent, a scripted copy with a scratch copy of the graph): Run stays enabled and starts; it ends after about 55 ms; the nodes of the application's engine never run and show no state and no error. The run's error was kept only where `ui.inspect` reports it. A silent failure.
- Cause (proved by experiment): a value one engine passes to another is labelled `node.port`, and the engine SDK split that label at its last dot (`crates/engine/src/bind.rs`, with a comment that port names never hold a dot; the Python SDK the same). With a dotted port the node came out wrong, and the application's engine refused that whole part of the run for an unknown node. A driver across engines is not the fault: the text engine's range drives the array engine's node, and listing both engines in the graph changed nothing. The controller's first suspicion, the driver belonging to another engine, was wrong.
- Issue: none
- Fixed: 2026-11-24, branch `fix/b162-range-driver`:
  - **The split:** `bind::split` (Rust) and `split_key` (Python) cut the label at the dot that leaves a node the graph has, the longest when more than one fits. `docs/interface.md` 7.6 rule 10 says so, and that ports and node ids may hold dots.
  - **Said aloud:** a run that stops as a whole now puts "the run stopped: <why>" on the status line, which is every shell's banner (`crates/editor-core/src/document.rs`).
  - **Tests:** a dotted port and a dotted node id in each SDK, and the status line, each failing without its fix; a coordinator test of a range in one engine driving a setting in another.
  - **Launched** (the application on the Mac, scripted, a scratch copy of the maintainer's graph, with the application's own matching fix): all 26 nodes done, the range and the driven layer among them; before the fix, the error above.
  - **Not checked:** the new status-line message in a window (the fixed application no longer fails; a test covers it); the iPad and Windows.

### B159. Mac: stepping through the items of a result does not keep the picture's zoom and view

- Reported: 2026-11-24
- Report: "scrolling through items in result does not maintain zoom and view of image."
- Context: the maintainer's hand use of an application built on the Mac shell from the dev branch at `0147266`, while B158 was being worked on.
- Answers (the maintainer, 2026-11-24): asked which window and control, and what the picture does on the next item, they answered: "Color pencil, final result, in the pop-out window. Zoomed out to icon size for image". So: the pop-out of the final result of a colour-pencil graph; the picture comes out zoomed out to about the size of an icon.
- Then: "I zoomed in to 200%, next view was more like 10%". So the next item neither keeps the zoom nor fits the window: it shows far smaller than either.
- Issue: none
- Cause (an agent, reproduced on a scripted Mac copy through the control port, with pictures 2,400 and 1,200 wide):
  - **Not kept:** the pop-out rebuilds the picture's view for every item, and the zoom lived only in the old view. The pinch zoom is the shell's, by `docs/shell-interface.md` 13.2.1, and the shell threw it away.
  - **Far smaller than fit:** the next item's picture can arrive before its new view has a size. The fit was then worked out as 0 and held at the view's smallest magnification, 5%. It is a race, and it happens when the picture is already in the shell's cache. The iPad had the first fault only.
- Fixed (Mac, iPad, iPhone): 2026-11-24, branch `fix/b159-popout-zoom`:
  - **The rule:** within one pop-out, a zoom and view once made stay through every item and every new value, kept in the picture's own terms (points per value pixel; the centre as a fraction of the picture). An item of another size gets the same zoom about the same relative point. With nothing kept the picture fits. Asking for Fit or 1:1 forgets the kept zoom. No zoom is applied before the view has a size.
  - **A command gap closed:** `viewer.zoom {to, at}` was taken by the core and ignored by the native shells. `popout.get` now carries `zoomAsk {seq, to, at}` and a shell applies each ask once.
  - **Tests:** three in the Swift kit, an iPad UI test that fails without the fix (item 2 at 0.585 where 2.0 was asked), and a conformance case.
  - **Launched** (a scripted Mac copy, control port only): zoomed to 200% about a point, then stepped: item 2 (the same size) and item 3 (half the width) both read back 200% about the same relative point; Fit asked gave fit again. The same on iPad and iPhone simulators.
  - **Not checked:** a person's own pinch and pan (the tests ask for the zoom by command); a device; the private graph of the report.
  - **Not done:** Windows loses the zoom on every item and refresh, falling back to fit (read from its code); egui keeps its zoom only while an array's shape stays the same.

### B151. About (and Help) do not show the build's date or id

- Reported: 2026-11-24
- Report: "add build date and id to help/about"
- Context: the maintainer tried Undo in the curve editor of an application built on the Mac shell and could not tell which build they were running: "undo on curve failed, and I want to make sure I was running the right one". Two builds were on the Mac with the same name, bundle id and version, one with the B150 fix and one without; the copy running at the time was the one without it.
- Fix (the maintainer, 2026-11-24): asked "Which id? I'd show the shell's git commit, plus the application's own commit when one is built on the shell" and "Which shells? I'd do all of them (Mac, iPad, iPhone, Windows), since the same confusion can happen on the iPad", they answered: "Yes to both on b151". Then: "fix B151".
- Issue: none
- Fixed (core, egui, Mac, iPad, iPhone): 2026-11-24, branch `fix/b151-build-id`. Note: `claude-debug/b151/b151-build-id.md`.
  - **One source:** `app.about` now also returns `build {line, shell, app, ui}`. `shell` is the core library's commit, whether its tree had uncommitted changes, and its build time; `app` is the same for an application's own library, when it supplies them; `ui` is the native app's own, passed at start as `app.built`.
  - **The line:** for example "Built 2026-11-24 22:14 UTC · shell 22f5884+changes". `+changes` marks a tree with uncommitted changes; the time is the newest of the three, in UTC, since the core writes the line.
  - **Captured at build time** by a new crate, `plumcraft-build-stamp` (its build script runs git once; with no git every fact reads "unknown" and the build succeeds), and by `native/apple/stamp-build.sh` for the Swift apps. Nothing runs git at run time; no path or name is stored.
  - **Shown:** first in the Mac's About panel, in the iPad's and iPhone's About (selectable, with Copy Build), and in egui's About. On the iPad with a keyboard, Help now starts with About.
  - **An application built on the shell** calls `plumcraft_build_stamp::capture::emit` in its build script and `plumcraft_ffi::app::set_build` before it opens the core.
  - **Tests:** six in the stamp crate, the reply's shape through the C ABI, two in the Swift kit.
  - **Launched** (an agent; scripted Mac copy, iPad and iPhone simulators): the line read back from `app.about` and seen in each About. The controller looked at the Mac's picture.
  - **Limits:** the library's stamp does not notice an edit outside `crates/` and the cargo files; the Swift app's own stamp covers the rest.
  - **Not checked:** an application's own stamp in a real application (a test sets a made-up one); the iPad's Help item was not opened; nobody copied the line by hand.
  - **Not done:** Windows (its About must show `build.line`, and its build must stamp `app.built`).

### B160. Windows: a graph saved as "person check.json" is created as "person check.json.plum" and then not found

- Reported: 2026-11-24
- Report: "Windows: \"person check.json\" was created as \"person check.json.plum\" and is not found"; then "Windows hung on \"person check.json\" not found"; then "fix B160".
- Context: seen by the maintainer on the Windows test VM's desktop while an agent's person check ran against the dev branch at `0147266` plus the uncommitted B154 Windows work. FR2, merged the same day, made `.plum` the first kind the core offers; FR2's hand-off list for Windows named this step as one to check.
- Cause (an agent, read back from the running app): the core offers the right kinds, `.plum` then `.json`. The Windows shell answered a graph's save question with the WinRT `FileSavePicker`, which appends the selected type's extension to a typed name, so `person check.json` came back as `person check.json.plum`; that picker also creates the file itself, empty.
- The box that stayed up: the person check's reopen step then typed a name that did not exist. The system's "File not found" box belongs to the picker's own host process, which outlived the app the run stopped. No wait in the script was without a limit.
- Issue: none
- Fixed: 2026-11-24, branch `feat/b154-s1-windows`:
  - a graph's Save As uses the common item dialog (`PathDialogs`), which does not create the file and starts in the graph's folder;
  - `PathBrowse.KeepTypedExtension` keeps a typed name whose extension is one of the offered kinds' (a unit test);
  - the person check's reopen step closes its own Open dialog when it fails.
  - **Launched** (the person check on the Windows test VM's desktop, the maintainer's session): the save step writes `person check.json` and the reopen step opens it; every step passed.
  - **Left:** a graph's Open still uses the WinRT picker, whose host can outlive the app.

### B158. Mac: the wire does not follow a dragged bend point, and bend points show large on unselected wires

- Reported: 2026-11-24
- Report: "The wire does not drag with the bend points, and the points are large/visible even when the wire is not selected."
- Context: the maintainer's first hand use of B155, in an application built on the Mac shell from the dev branch at `0147266`.
- Answer (the maintainer, 2026-11-24): asked whether the wire jumps on release or never moves, they answered: "fix B158 (jumps on release)".
- Cause (an agent, read from the code and reproduced in process):
  - **The wire:** during a bend-point or route drag the Apple canvas moved only a ring; the wire was redrawn when the drag's `wire.route` ran at the end, and the move's reply carried no path. An ordinary bend-point drag had this before B155 too.
  - **The points:** the canvas drew a radius-5 ring at every bend point into each wire's own path, always.
- Issue: none
- Fixed (core and Apple shells): 2026-11-24, branch `fix/b158-wire-drag`. Note: `claude-debug/b158/b158-wire-drag.md`.
  - **The wire follows:** each move of a bend or route drag answers `update.wire {nid, via, path}`, the path built by the scene's own path code (`plum_drag_wire` in the C ABI). The canvas draws it in place of the wire until the scene brings the new one; a cancel puts the wire back at once. The shell computes no route.
  - **Quiet points:** a dot of radius 3 shows only while the wire or one of its bend points is under the pointer, while the wire has an end on a selected node, or while it is being bent. The scene has no wire selection, so "selected" means an end on a selected node. The hit radius is unchanged.
  - **Tests:** `crates/editor-core/tests/wire_drag_preview.rs` (4), a C ABI test, two Swift canvas tests, and a shared conformance case.
  - **Launched** (a scripted Mac copy through the control port): no dots on an unselected wire, small dots when its node is selected, and the move's path equal to the scene's path after the drag.
  - **Proven by hand** (the maintainer, 2026-11-24, in an application built on the Mac shell from the dev branch at `22f5884`): "Drag works fine now."
  - **Not checked:** the live preview in the running app. A gesture sent through the control port does not pass through the canvas's own drag, so it was read in process only. No hand drag, no device.
  - **Not done:** egui still draws a handle at every bend point, always (its wire does follow a drag). The iPad has no hover, so an unselected wire's bend points can be taken but not seen. Windows: nothing written.

### B155. A wire's automatic 90-degree bends can't be interacted with

- Reported: 2026-11-24
- Report: "There is also no way to interact with the 90 degree automatic bends."
- Context: said with B153's second report, in the maintainer's hand use of an application built on the Mac shell (the dev branch at `487a40d`). The bends are the corners of a wire the editor routes itself.
- Design (the maintainer, 2026-11-24): asked what the interaction should be (the controller proposed making a corner or a segment draggable, which turns the automatic route into the person's own bend points), they answered: "B155: Look at converting bends <-> right angles and allow dragging." Then: "fix b155".
- What the look found: no editor here ever let a person take hold of an automatic corner; egui has Straighten and nothing else.
- Issue: none
- Fixed (core and Apple shells): 2026-11-24, branch `fix/b155-wire-corners`. Design note: `claude-debug/b155/b155-wire-corners.md`.
  - **No new stored state.** A wire still has only its bend points. A wire whose bend points (two or more) each sit straight across from, or straight above or below, the one before is drawn with straight legs and rounded corners; any other wire is drawn as before.
  - **Dragging:** a press on a corner or a leg of an automatic route is a hit (`wire`, part `corner` or `segment`); the drag (kind `route`) makes the route's corners the wire's own bend points with the dragged one moved, in one `wire.route`, so one undo step. A leg moves only across itself; the legs touching the pins can't be taken.
  - **Converting:** the wire's menu gains Make Right-Angled (`wire.square`, a new core command), offered only where it would change something; Straighten is renamed Route Automatically (the same `wire.route` with no points).
  - **Tests:** `crates/editor-core/tests/wire_corners.rs` (6) and a shared conformance case every shell's runner must pass.
  - **Launched** (an agent, a scripted Mac copy through the control port, no mouse): a corner drag, a leg drag and Make Right-Angled each read back as the expected bend points; three undos restored both wires.
  - **Open decision:** an existing wire whose own bend points happen to line up exactly is now drawn right-angled where it was a curve. The other way is a stored per-wire style, a graph-format change.
  - **Not checked:** a real mouse or finger drag; a device.
  - **Not done:** egui draws a right-angled wire and has the two menu items, but can't drag an automatic corner (its canvas handles wires itself). Windows: nothing written; it needs rings at bend points, the drag on a corner or leg, and drag code 9. Automatic corners show no handle until dragged.

### B156. Mac: after grouping nodes, no group header appears and the group can't be collapsed

- Reported: 2026-11-24
- Report: "after grouping nodes, no group header appears, no way to collapse group."
- Context: the maintainer's hand use of an application built on the Mac shell (the dev branch at `487a40d`).
- Answers (the maintainer, 2026-11-24): asked how the group was made and what appears afterwards, they answered: "^A, Group->group; there is a background but no indication of header status.  This occurs after opening oil-painting-3.json". So: every node selected, then the Group menu's Group item, on a large style graph of the research software; the group's background is drawn, with no header.
- Cause (an agent, reproduced through the control port): the Apple canvas drew the whole header strip, colour and name, as one picture the group's full width, and its picture maker draws nothing wider than 16,384 pixels (`Labels.swift`). At the Mac's drawing scale that is 4,096 graph units, so any open group wider than that lost its header while its background and outline still drew. A three-node group 878 units wide had its header; the same nodes spread over 5,478 units did not. The core's scene was right throughout. The iPad and iPhone use the same canvas and had the same fault.
- Issue: none
- Fixed: 2026-11-24, branch `fix/b156-group-header`:
  - **The header:** `LayerCanvas.placeGroup` draws the strip as a plain coloured layer of the group's width, and the name as a picture at most 1,024 units wide inside it.
  - **Collapse in the menus:** the Group menu gains "Collapse Group" (members of an open group selected) or "Expand Group" (a collapsed group selected), disabled with a reason otherwise; the header's own menu gains Collapse. Both run the existing `group.collapse`, one undo step (`crates/editor-core/src/menu.rs`).
  - **Tests:** `anOpenGroupsBandDrawsAtAnyWidth` (the canvas kit; fails without the fix) and `the_group_menu_collapses_and_expands_the_selected_group` (the core).
  - **Launched** (an agent, scripted copies through the control port, no mouse or keys): Select All then Group on three nodes spread over 5,478 units and on a scratch copy of the maintainer's graph (91 nodes, a group 9,400 units wide): the header strip is drawn; collapse, undo and expand work. Mac conformance 71 of 71; iPad simulator 60 of 60.
  - **Not checked:** a person's click on the header; the iPad's UI tests; a device.
  - **Left for Windows:** the header does not vanish there, but a large group's name and outline are drawn into one picture held to 4,096 pixels and stretched, so they blur.

### B153. Adding a bend point to a wire does not add one

- Reported: 2026-11-24
- Report: "adding a bend point does not add one; when adding one to an auto routed r->l connection removes auto routing, but does not add one either"
- Context: the maintainer's hand use of an application built on the Mac shell. Two cases in the report: an ordinary wire, where nothing is added; and a wire routed automatically from right to left (its source to the right of its target), where the automatic route goes away and still no bend point appears.
- Answers (the maintainer, 2026-11-24): asked how the bend point was asked for and whether Undo afterwards takes something back, they answered: "Right-click menu; undo does causes a redraw".
- More (the maintainer, 2026-11-24, in a later build from the dev branch at `487a40d`): "Adding the bend point places the point mid point of the selected segment, should place near the selection point."
- Cause (an agent, with a test that follows the shells' path): the place of the right-click never reached the menu. The core's `context` gesture put the click point (`at`) only into the menu it built itself; the native shells pass the reply's `context` to `menu.get` themselves, and that context had no `at`. Add Bend Point then fell back to a point of the core's choosing, midway between the two nodes: on a straight wire that can lie on the wire's old line or under a node, so nothing seemed to change; on a right-to-left wire it replaced the automatic route. egui adds the bend itself at the click and never had the fault. That both reports are this one fault is inferred for the first (the maintainer's graph was not at hand).
- Issue: none
- Fixed: 2026-11-24, branch `fix/b153-bend-point`:
  - **The core:** a wire's `context` in the gesture's reply carries `at`, the press in graph units (`crates/editor-core/src/gesture.rs`); `menu.get` already read it. No shell code changed, so the Mac, iPad, iPhone and Windows shells all send it now. `docs/shell-interface.md` says so.
  - **Tests:** `crates/editor-core/tests/wire_bend.rs` follows the shells' path (the gesture, `menu.get` with the reply's context, the item's command) on an ordinary wire and on one routed right to left: before the fix the points came out at (360, 40) and (410, 40) for presses at (418, 43) and (430, 104); after, at the presses. The conformance case for a wire's context checks that `at` is there.
  - **Launched** (an agent, the Mac's scripted copy through the control port, no mouse or keys): bends read back at the presses on both kinds of wire, each drawn with its ring. iPad simulator: the conformance cases and the UI tests passed.
  - **Not checked:** a right-click by a person; a device; Windows.
  - **Left for Windows:** its canvas draws no ring at a bend point, so a bend on a straight segment is invisible there and has nothing to drag.

### B152. Mac: the output of a histogram node can't be viewed

- Reported: 2026-11-24
- Report: "can't view output of histogram"
- Context: the maintainer's hand use of an application built on the Mac shell (the build with the B149 and B150 fixes). The node is the research software's Histogram node (inputs `array`, `bins` 256, `low` 0.00, `high` 1.00; output `histogram · ARRAY`), run and cached. Its value window opens titled "Histogram · histogram", says "table 256×2 float32", with the view chooser on "Table", and shows in place of the table: "This table can't be shown: not a table: expected value at line 1 column 1". Screenshot: `claude-debug/b152-histogram-table.png`.
- Cause (by reading, then confirmed by the fix's launch): the application's engine describes an array it marks as a table (`Form::Array`'s `table`) with the media kind Table, and its bytes are the array's numbers. The core's table reader (`plumcraft_media`'s `Want::Table`, `Table::from_bytes`) read only the core TABLE type's JSON, so the first byte of a float was "not a table". egui offers its Plot viewer first for such a value, which hid the fault there; the native shells offer only Table.
- Issue: none
- Fixed: 2026-11-24, branch `fix/b152-table-array`:
  - **The core reads such an array as a table.** `Table::from_array` (`crates/media/src/table.rs`): the last axis is the columns, named by the array's `names` when there is one a column; integers stay integers; every size is checked before a byte is read. The media service's table and sort derivations (`table_of`, `crates/media/src/service.rs`) and Copy and Export as CSV (`crates/editor-core/src/export.rs`) use it when the value's form is an array marked as a table.
  - **No Tree viewer for it.** `offered` (`crates/editor-core/src/viewers.rs`) no longer offers Tree for a table that is an array: it has no JSON to outline.
  - **Every shell:** the change is in the core, so the iPad, iPhone and Windows shells get it. Launched on the Mac only.
  - **Tests:** `an_array_marked_as_a_table_reads_column_by_column` and `a_hostile_array_is_refused_before_a_byte_is_read` (media), `an_array_marked_as_a_table_shows_in_the_table_viewer` (the core).
  - **Launched** (an agent, a scripted copy of the application built on this branch, through the control port, no mouse or keys): a picture, one channel of it, the Histogram node; its value window read back status ready, 256 rows, columns `value` and `count`, first row 0.001953 and 12283; sorted by count both ways; Copy gave 257 lines whose counts add to 656,096, the picture's 812 × 808 pixels; Export wrote a 257-line CSV.
  - **Not fixed:** egui's own Table viewer still says "no table" for such a value (its Plot viewer shows it).

### B150. Mac: Undo does not take back a change made in the curve editor

- Reported: 2026-11-23
- Report: "^Z /Cmd Z does not undo a curve change"
- Context: the maintainer's hand use of an application built on the native Mac shell (dev branch at `921fd65`'s Mac code), in the curve editor's window. So a person's drag on the curve does change it (the Mac person check's drag step, which sends no edit, is a fault in the step or in how it reaches the view), and Undo in that window does nothing. The mask editor's window had the same fault, fixed for its AppKit canvas; the curve window was given the same answer but never checked with a real drag.
- Issue: none
- Cause: a core window's SwiftUI content (the curves editor, the generic value window, Parameters, Compare) has no responder of its own for Undo and Redo, so Edit › Undo and ⌘Z reach the window, which answers them with its undo manager: AppKit's own, empty. Undo was greyed and ⌘Z did nothing. The mask window was spared because its AppKit canvas answers first.
- Fixed: 2026-11-23, branch `fix/curves-mac-drag` (uncommitted). The core windows' delegate gives each one an undo manager that is the document's history (`CoreUndoManager`, `Popouts.swift`): Undo and Redo are enabled and named as the core says, and run `edit.undo` and `edit.redo`. The person check now drives the curve drag through the window's own event routing (it went to the view directly, which no SwiftUI gesture sees), then presses ⌘Z as a key event through the app's routing.
  - **Checked** with the screen unlocked: without the fix, ⌘Z and Edit › Undo fail; with it, ⌘Z took the drag back in 4 runs of 4, and Edit › Undo in 1 run of 1 where it was tried right after the drag.
  - **Not yet shown:** ⇧⌘Z (Redo) sent as a key event did nothing in 4 runs of 4. It is not known whether the step's key event or Redo itself is at fault.
  - **The iPad:** its curve editor (a sheet) has no Undo of its own. A person closes it (Done) and undoes on the canvas. Read from the code and its UI test; not tried on the simulator.
- Redo (the maintainer, 2026-11-24, by hand in a build with the fix): "undo.redo both work". The fix's own test never proved Redo; this is the proof.

### B149. Mac: a loaded picture shows "the picture is missing" in its view and no preview on the node

- Reported: 2026-11-23
- Report: "Loading an image fails", with a screenshot: a picture source node ("The plum photo", path set, align on) ran green; its value window's header says "array RGB 812×808×3 float32" with the per-channel minimum, maximum and mean, but the picture area is empty with the words "the picture is missing", and the node's own preview is a blank square.
- Context: the maintainer's first hand use of an application built on the native Mac shell (dev branch at `2d0d106`), opened as itself, not as a scripted copy. So the array was read and described; what is missing is its picture. The screenshot is kept at `claude-debug/b149-mac-picture-missing.png` (untracked). Scripted runs of the same build showed previews.
- Cause (reproduced by an agent with a scripted copy started with an empty environment, 2026-11-23): the Mac shell gives the core its store folder but does not set `COMMONKIT_STORE_ROOT` to it, as the iPad shell does. Started from the Finder, an engine that stores values through commonkit writes them under commonkit's default folder while the core looks in its own, so the value is "missing". Scripted runs set both to one folder, which hid it. Any engine that writes through commonkit is affected, in Plummer too.
- Also found: the core's reason ("the value's bytes are gone: run again to see it") is sent in a field the Mac view does not read, so the person sees only "the picture is missing".
- Fix (proposed, not made): set the variable to the core's store before the core opens, when it is unset; read the core's reason; a test that starts the app with an empty environment and checks a loaded picture's preview.
- Issue: none
- Fixed: 2026-11-23, branch `fix/b149-mac-store-root` (uncommitted):
  - **The store:** `ShellCore.open` (the Apple kit, so the Mac and the iPad) settles one store before the application's engines are registered (`ShellCore.store`): a `COMMONKIT_STORE_ROOT` already set is kept and is the core's store too; unset or empty, it is set to the app's store. A scripted Mac launch may now leave it unset, as the Finder does; the store it then gets is checked to be under scratch like every other place it writes.
  - **The reason:** the C ABI's `media.get` gives a `missing` value a `reason` ("the value's bytes aren't in this app's store: run again to see it"), the field the Mac, iPad and Windows picture views already show. The reason quoted in the report came from `viewer.data`, another reply.
  - **Correction to the cause:** PlumCraft's own in-process engines are handed the core's store, so Plummer itself was not affected; only an application's engine that stores through commonkit itself was.
  - **Checked:** the Mac person check's new step "B149: launched as the Finder does with no store variable…" (`finder-store-check.py`: an empty environment with no store variable, the working folder `/`, and `HOME` a scratch folder so that commonkit's default store is scratch too) passes on Plummer. Given an application's Load Image graph, the same step passes on the application built on this branch, and fails without the fix ("missing", with the new reason; the bytes were in commonkit's default store). Swift `theCoreAndItsEnginesShareOneStore`, Rust `a_missing_value_says_why_in_its_reason`. Mac conformance 71 of 71; the person check otherwise as on the dev branch (the same 3 failures, with the screen unlocked, on `2d0d106` unchanged: S4a's drag and its Undo, and File › Close). iPad conformance 60 of 60 and 11 UI tests passed on the simulator.
  - **Not checked:** the application opened as itself by the Finder (the maintainer reopens it); Windows, where the same store fault does not occur (its `ShellConfig.cs` always sets the variable) and whose picture view reads the same `reason`, so it needs no change.

### B146. iPad: the gap between a wire's name label and an input pin is too large

- Reported: 2026-11-23
- Report: a screenshot marked by hand "Gap too large", with an arrow at the space between the label "said" and the `text` input pin of a Reverse node. On the same screenshot the label "said ×2" sits close to the `text` output pin of the node it comes from.
- Context: the maintainer's hand use of the Release build of Plummer 0.7.0 on their iPad, installed from the dev branch at `ea051db`. The picture is kept at `claude-debug/b146-label-gap-ipad.png` (untracked).
- Design (the maintainer, 2026-11-23): asked "should the gap on the input side shrink to match the output side as it looks in your picture, or do you want both changed?", they answered: "Both." So both sides' gaps change. By how much was not said; the controller's reading is that both become the same small gap, the input side's being the one marked too large.
- Found (2026-11-23): the two sides are one rule already. Both tags sit the core's stub from the pin's centre: a driver's tag starts 14 points right of its output pin, a receiving tag ends 14 points left of its input pin (`NET_STUB`, the core's scene, which every shell draws by). The maintainer's picture measures the same gap on both sides (about 23 pixels each, 12 points), so the input side's looking too large was not a difference between the sides
- Fixed: 2026-11-23, branch `fix/b146-label-gap`. "Both": the one stub is now 8 points, a small gap beside a pin's square on both sides (the controller's reading of how much: the same small gap). It is held in one place, `plumcraft_session::layout_cmds::NET_TAG_STUB`, which the core's scene (`NET_STUB`), the tidy layout's spacing and the egui editor (`net_view::STUB`) all take, so the egui editor, the Mac, the iPad and Windows move together. A tag a person has moved keeps its own place. The shared scene fixture (`crates/editor-core/fixtures/scene/full.json`) has the new offsets; no shell's test expected the old value. Pictures before and after on the iPad simulator, in landscape, at the default text size and one step larger: `claude-debug/b146-label-gap.md`. Not checked: a device; Windows (its tests read the fixture; the Windows agent runs them)
- Issue: none

### B147. iPad: the app starts in a folder it cannot write to

- Reported: 2026-11-23
- Report: "iPad started In unwritable folder. Files / Plummer seems a better starting place"
- Context: the same hand use as B146, the first launch of the build from `ea051db`.
- Fix (the maintainer's suggestion): start in the app's own folder, the one the Files app shows as Plummer.
- Detail (the maintainer, 2026-11-23): asked what showed the folder could not be written to, "failed save"; asked whether this was Plummer (not the application built on it), "yes".
- Found (2026-11-23, the iPad simulator, a fresh install each time): the first launch opens Showcase from the app's own folder, and the document browser it goes back to opens in that folder too (Plummer: Samples and New Graph). But the browser's New Graph makes the graph wherever the browser stands. Pressed with the browser in On My iPad (reached from its sidebar), the dev branch's app made `Untitled.json` in On My iPad's own storage, outside the app's container (`…/File Provider Storage/Untitled.json`), and opened it there in place. A failure to make it was only written to the log (`failedToImportDocumentAt`), never shown. On the simulator the save to that outside place worked: the "failed save" itself is the device's and was not reproduced (a place the app may not write beside the file, such as On My iPad's top folder or another provider's, or a start folder remembered from an earlier install's container)
- Fixed: 2026-11-23, branch `fix/b147-ipad-start-folder`, the iPad and iPhone app (PlumCraftPad), as the maintainer suggested. New Graph always makes the graph in the app's own folder (Documents, which the Files app shows as Plummer; "Untitled.json", then "Untitled 2.json" and on), wherever the browser stands, and opens it there. The browser shows the app's folder when the app starts. A new graph that can't be made, and a save that fails, now tell the person why and where ("Couldn't make a new graph — In Plummer (this app's folder in Files): …"; "Couldn't save Untitled in Plummer: …"). A file ask's remembered start folder that no longer exists falls back to the app's folder. Named test `testANewGraphIsMadeAndSavedInTheAppsFolder` (`PlumCraftPad/UITests/FirstLaunchTests.swift`, run from a fresh install by `claude-debug/n4-decided-2/fresh.sh`): first launch, the browser, On My iPad, New Graph, a node added, and the saved file in the app's Documents holds it; it failed on the dev branch (the graph made outside the app's folder) and passes with the fix. Report: `claude-debug/b147-ipad-start-folder.md`. Not checked: a device (the failed save itself); New Graph stays the system browser's own button, answered by the shell, as it was
- Issue: none

### B133. iPad and iPhone: a path input offers no system file browser

- Reported: 2026-11-23
- Report: "Widget for path doesnt provide a system dialog for file browsing. Guessing paths on the iPad is not good UX"
- Context: the maintainer's first hand use of the release build of Plummer 0.7.0 installed on their iPad and iPhone (from the dev branch at `2c3de06`). A path input on a node's face is edited as plain text there. Floor row F16 (path hints) already says "Partly", for want of a Browse button, on the Mac and the iPad.
- Open question: on iPadOS an app reaches a file outside its container only through the document picker, which hands back a security-scoped URL, not a path an engine can open later. Should Browse copy the picked file into the app's folder and use that path, or keep a bookmark to the original?
- Open question: Mac and Windows too (the same row, with the system's open panel), or the iPad and iPhone first?
- Decided: 2026-11-23, the maintainer, on the two questions (the controller recommending "start with copying" and "all three together, since the button belongs in the core, with each shell supplying its own picker"): "Yes, fix b133 on all systems." The copy-or-bookmark question was not answered separately: copying is built, and stays an open decision
- Built (branch `fix/b133-path-browse`): the Browse button is the core's. A path row that can be edited (its widget has `pathMode`) has it at the right end of its field, a square the row's height (`geometry::browse_rect`), which every shell draws; a tap there is the core's gesture `row.browse {node, port}` (a command, invoked like Save Result…: `ui.invoke`, `command.run` and the gesture all ask), which closes any open field editor without its text and raises the core's existing path ask (`file.open`, `file.save` or `folder.pick`, purpose `path`, the hint's filters as `kinds`); the answer is set as typing sets it (one undo step). The Mac answers with its open or save panel (the real path). The iPad and iPhone copy a picked file into Documents › Picked Files (the Files app's On My iPad › Plummer › Picked Files) and answer with the copy's path: kept by its own name, the same bytes again reuse that copy, another file of the same name is numbered ("notes 2.txt"), earlier copies are left alone; a folder (`dir`) is opened in place with access for the graph's life, as before (not lasting across a relaunch); a may-be-new path (`save`) is the existing alert naming a file in the graph's folder (or Documents). The field editor on a path row stops short of the button, so it can be tapped while the text is edited. egui keeps its own "…" button, which calls the same core `browse`. Windows: its half to come on the same branch
- Built (branch `feat/path-photos`, 2026-11-23, for an application's picture inputs): on the iPad and iPhone a path that takes pictures only offers Photos as well as Files (an action sheet, then the photo library's picker); the photo is kept in Picked Files as it is, or converted to the PNG or JPEG the path takes, and its copy's path is the answer. No core command: the choice is part of answering the one `file.open` ask (`docs/shell-interface.md` 11.3, with the alternative of a `sources` field in the ask, for the maintainer). Decided 11A and 12A (2026-11-23): the shell's business only; launched by the test engine's `ports.picture` (a path that takes PNG and JPEG only) in `PhotosTests.swift` on the iPad simulator: Browse, the choice, Files (cancelled), Photos, a photo of the simulator's library picked, the node's path a copy in Picked Files that exists
- Built for Windows (2026-11-23, branch `fix/b133-windows-browse`):
  - **The button:** the path row draws Browse in the core's square (`PathBrowse.Square`, as `geometry::browse_rect`). UI Automation sees it as a "Browse" button, and the tap is the core's gesture.
  - **The dialog:** the path asks (purpose `path` and `files`) are answered with the system's common file dialog (`PathDialogs`):
    - it opens in the current value's folder, else in `startIn`;
    - it has the hint's kinds as filters;
    - it gives back real paths, so nothing is copied.
  - **The field editor** stops short of the square.
  - **Browse with the field open** closes the field without its text: the focus loss the press causes commits nothing over Browse.
  - **Checked by:** person check step 20 (the dialog read through UI Automation and cancelled, pictured); the conformance case on ARM64 and x64; `PathBrowseTests`.
- Remaining after Windows: the iPad's Photos source (another agent's).
- Issue: none
- Fixed: 2026-11-23, on every shell: the core and the Apple shells (#179), Windows (#182), Photos as a source on the iPad and iPhone (branch `feat/path-photos`). Photos is checked on the iPad simulator only; no device and no iPhone run has picked a photo.

### B145. Windows: a value of several lines showed and committed only its first line

- Reported: 2026-11-23, found by the agent on branch `native/n5-editor-row`; not reported by the maintainer.
- Report: the value window showed "[" for a curve's JSON, and Apply would have sent "[". The same happened in the row editor's multiline field, the inspector's multiline widget and a form's text field.
- Cause: each `TextBox` was given its `Text` before `AcceptsReturn`, and one that does not yet take returns keeps only the first line.
- Fixed: 2026-11-23, branch `native/n5-editor-row`, with `AcceptsReturn` set first. Person check step 21 reads the whole value from the value window; the other three fields have the same fix but no launched check.
- Issue: none

### B141. Windows: a row field's focus loss put back the value it opened with (B129 on Windows)

- Reported: 2026-11-23, an agent, while checking B129 on Windows
- Report: the Windows row editor's `LostFocus` committed the field's text whatever it was, so a value set while the field was open was overwritten.
- Fixed: 2026-11-23, branch `native/n5-lists`: `RowEditor.FocusLostAsync` commits only typed text; test `AFocusLossCommitsOnlyWhatWasTyped`.
- Issue: none

### B142. Windows: no tooltip on a pin, so a pack type's full id could not be read

- Reported: 2026-11-23, an agent
- Report: the Windows canvas showed no row `hover`.
- Fixed: 2026-11-23, branch `native/n5-lists`: a pin shows it as a tooltip; person check step 19.
- Issue: none

### B136. iPad: a script's `graph.open` leaves the window on its old file

- Reported: 2026-11-23, an agent (an application's iPad app, a conformance case in the simulator)
- Report: `graph.open` over the control port, in a document the browser had opened, replaced the graph and moved the core's file and title, but the window kept the browser's file: the title still named it, and Save when closing a dirty document would have written the opened graph over the window's file. A person's File › Open on the iPad goes to the document browser, so only scripts reached it.
- Fixed: 2026-11-23, branch `native/pack-views`: the title follows the core's file when it differs, and Save on closing writes to the core's file (or a `.json` beside a picture), never over the window's other file.
- Issue: none

### B135. iPhone, landscape: the parameters dialog's sliders take the whole width

- Reported: 2026-11-23
- Report: "parameters dialog in landscape mode on iphone uses whole width for sliders. Needs attention."
- Context: the maintainer's hand use of the release build of Plummer 0.7.0 on their iPhone (dev branch at `2c3de06`), turned to landscape. The phone layout has never been run by a test or looked at.
- Open question: what should it be instead: a column of a readable width centred or at one side, with the canvas still in view beside it?
- Decided: 2026-11-23, the maintainer, on the controller's proposal ("a column of readable width at one side, with the canvas still visible beside it, the way the iPad's inspector works. That way you can watch the result change as you drag a slider"): "Fix b135 with that"
- Added: 2026-11-23, the maintainer: "Check other landscape presentations for similar parameter display issues". So every presentation that shows parameters or controls is checked in landscape on the iPhone (and the iPad where the same code applies), and those with the same fault (controls stretched across the whole width) are fixed with the same width rule, held in one place
- Fixed: 2026-11-23, branch `fix/b135-phone-params-landscape`. One rule, `ReadableColumn` (PlumCraftPad): when the window's height is compact and it is wider than tall (an iPhone turned), the parameters are a column at the trailing edge, 45% of the window's width (at least 320 points, at most 420, the iPad inspector's width), with the canvas live beside it; otherwise a popover in regular width and a sheet in compact width, as before. Turning the phone moves the parameters between column and sheet with any edit in progress set first (B129). The UI test `testParametersBesideTheCanvasInLandscape` (iPhone simulator, one step larger text) checks the column's width against the window's, taps a face beside it, drags a slider and sees the value change, and types a label and turns the phone. The other presentations walked in landscape: `claude-debug/b135-phone-params-landscape.md`
- Issue: none

### B134. iPhone: no obvious way to leave the Add Node menu

- Reported: 2026-11-23
- Report: "no obvious way to escape add node menu on iphone"
- Context: the maintainer's hand use of the release build of Plummer 0.7.0 on their iPhone (dev branch at `2c3de06`). The phone layout has never been run by a test or looked at.
- Context (2026-11-23, the iPhone 17 Pro simulator at one step larger text, branch `fix/b134-phone-add-node`): in compact width Add Node is the More menu's, and the palette is a page sheet. In portrait it opens at half height with a grabber and no button: a swipe down and a tap on the dimmed graph above it close it, neither of them obvious. In landscape (compact height) UIKit shows a page sheet full screen: no grabber, no swipe, nothing outside it to tap, no button: a dead end. The controller suggested a swipe down or a tap outside; the maintainer, verbatim: "No exit. Fix b134"
- Decided: 2026-11-23, approved for fixing ("Fix b134")
- Fixed: 2026-11-23, branch `fix/b134-phone-add-node`. In a sheet the palette has a Cancel button beside its search field (`palette-cancel`), and in compact height it stays a sheet on the bottom edge (`prefersEdgeAttachedInCompactHeight`), so a swipe down works there too; it opens at half height where the phone has the height for it. Every way out is the core's `canvas.palette {open: false}` (`PaletteModel.dismiss`): the UI test reads the palette closed in the core (`palette.open` false) and on screen after each. In landscape the sheet takes the whole height (UIKit's one height in compact height), so there is no graph above it to tap: Cancel and a swipe are the ways out there. Every other sheet the app shows on the phone (pop-outs, the parameters, Settings, the Messages window, the Cache Report and Parameters window, Compare, About) already has a Done button; the alerts have their buttons; menus close by a tap outside: none has the same dead end. UI test `testLeavingAddNodeInCompactWidth` (Cancel, a swipe down, and a tap above, each in portrait and landscape, on the iPhone simulator; and in compact width on the iPad). `PlumCraftPad/conformance.sh` has a named iPhone configuration (`PHONE=1`)
- Issue: none

### B130. On the iPad in a 375-point window, Run and More folded into the system's overflow menu (More inside More)

- Reported: 2026-11-22, the controller's device run at 375 by 655 in landscape (`testNoKeyboardWithNothingEdited` failed in 18 s, "the bar's More menu": the bar had Documents, the title "Inline Editing" and the system's `OverflowBarButtonItem` "More"; `testCompactWindowsByATap`, on a graph with a shorter title, had found More earlier in the run)
- Reproduced (branch `native/n4-prompts`, the simulator, iPadOS 26.5, Windowed Apps, 375 by 655 landscape): not at the simulator's default text size; at extra-extra-large text, every time. The editor style's leading title took the room, and UIKit moved Run and the app's More into its own overflow button
- Fixed: in a narrow compact window (under 440 points) at a text size above the default, the bar keeps More alone on its right and Run goes first into More's menu (`narrowRunInMore`); on iPadOS 27 the compact bar's right-hand items also get the bar's high visibility priority (`visibilityPriority`, new in 27). Tried first and not enough: the navigator style's centred title (the items still folded). On the simulator (26.5) at 375 by 655 and extra-extra-large text, `testNoKeyboardWithNothingEdited` failed before the fix (w2, w3, w4) and passed after it (w5, 20.7 s). The iPadOS 27 path and the device are not run here. Also seen at that text size, not the device's failure and not fixed: `testCompactWindowsByATap` doesn't find Settings in the longer More menu, and `testInlineEdits` doesn't open the node's menu from the more-button (both pass at the default size)
- Decided: 2026-11-22, the maintainer: "1,2 yes. I have text size in appearance/text size 1 slide position larger" (1: accept Run inside More in a narrow window at a large text size). Accepted as built; the iPad is one step above the default text size (`extra-large`), the size to check at
- Checked at that size (2026-11-22, branch `native/n4-b111`, the simulator; `TEXT_SIZE=one-step-larger` in `PlumCraftPad/conformance.sh` and the scratch scripts): at 375 by 655 landscape, with the fix switched off for one run, `testNoKeyboardWithNothingEdited` failed at "the bar's More menu" (the fold, as on the device); with it, it passed in every run after (five runs). At that size two other tests failed for reasons of their own, both fixed in the tests: `testInlineEdits` tapped the face's more-button where the status put it, off a 375-point window (now panned into reach first), and the export picker in a narrow window is a whole-window sheet with no Cancel (now closed by a pull from its top, never by Save)
- Issue: none

### B129. On the iPad with the test engine attached, the whole suite fails "a value typed into a row…": the value set goes back to what the field showed

- Reported: 2026-11-22, branch `native/n4-prompts` (the iPad conformance with `PAD_PORTS=1`)
- Report: in the whole suite (`conformance.sh all`) with the test engine attached over HTTP, the case "a value typed into a row is checked, held to its range, and is one undo step" fails at step 9: `row.set {text: " 7 "}` answers `value: 7`, and the next `graph.get` has `count` 3, the value the field was opened with. Three runs of three. It passes in the edit half alone with the test engine (29 of 29) and in the whole suite without it (59 of 59), and on the Mac with the test engine (66 of 66)
- Not known at first: what takes the field's focus. A guess, not checked (and wrong, below): the ports cases leave pop-outs open, which on a whole-screen iPad are windows of their own (scenes); a scene change while the row's field is open ends its editing, and the field commits the text it was opened with on losing focus (`FieldEditorHost.textFieldDidEndEditing`), after the script's value. If so, a person who changed a value elsewhere (the inspector, a script) while the field was open would lose it the same way
- Decided: 2026-11-22, the maintainer: "1,2 yes" (2: look into it now)
- Found (2026-11-22, branch `native/n4-b111`, the simulator): the field logged why its editing ended (`textFieldDidEndEditing`, with the call stack): it was still the key window's, the scene active, nothing finishing it, and the stack was UIKit's own: `GraphViewController.replace(with:)` dismissing a presented sheet to present another (`presentReplacing`, here a pop-out's sheet; the ports cases leave pop-outs open), and the dismissal giving focus back to the responder of before the presentation (`UIViewController _restoreInputViewsForPresentation` → `UIKeyboardSceneDelegate _restoreInputViewsWithId:animated:` → `becomeFirstResponder`), which took it from the field. The field's focus loss committed the text it was opened with ("3") over the value the script had just set (7). The guess above (a scene change) was wrong. Not a test artefact: a person whose value changed while its field was open (the inspector, a script, another window) would lose the change the same way
- Fixed at source (2026-11-22, branch `native/n4-b111`): a focus loss commits only what the person typed (`FieldEditorHost.lostFocus`); a field left as it opened sets nothing and stays open (the core still has it open). The whole iPad suite with the test engine: 65 of 65 (it failed 3 of 3 before). The Mac's field had the same rule (`controlTextDidEndEditing` committed what the field held) and gets the same change (`RowEditor+AppKit.swift`), though no Mac run had shown it
- Issue: none

### B125. Release blocker: the contact sheet's label font: verify its licence or replace it before a public release

- Reported: 2026-11-22, the controller
- Report: the engine SDKs drew contact sheet labels with glyphs extracted from Pillow's built-in default bitmap font (`ImageFont.load_default_imagefont()`, Pillow 13.0.0), stored as `crates/engine/src/sheet_font.json` and `python/src/plumcraft_engine/sheet_font.json`. Its upstream origin and licence terms were not verified. Pillow's `ImageFont.py` marks that data `# courB08`: Courier Bold 8, from X11. The maintainer: "Just use the Pillow font. We will deal with issues before Plummer is released to public."
- Context: THIRD-PARTY.md ("The contact sheet's label font"); note 24, "The label font, settled for now" and "The label font: Aileron". A second font was looked at and not taken: the Ubuntu Font Licence lets a rasterised table be redistributed with Canonical's notice and the licence, but its condition 2 names a modified version two opposite ways, and the text doesn't say which a one-size bitmap of ASCII is
- Fixed: 2026-11-22, branch `feat/sdk-save-4`: the font was replaced. The table is now rasterised from Aileron Regular (Sora Sagano, dotcolon, CC0 1.0), the font Pillow's `ImageFont.load_default(size)` embeds, at 11 pixels, by `claude-debug/sdk-save/make_sheet_font_aileron.py`. That script checks at run time that it got a FreeType font (Pillow 13.0.0, FreeType 2.14.3). The table holds 8-bit coverage, advances and offsets; both SDKs blend it with the same integer rule. No courB08 data remains. Checked: by the controller, 2026-11-22, dotcolon's page ("Aileron is No Rights Reserved", linked to CC0 1.0) and Pillow's `load_default` docstring ("a version of Aileron Regular, https://dotcolon.net/fonts/aileron, with a more limited character set"); by the agent, the same day, the embedded TTF's own `name` table: copyright "No Rights Reserved.", family "Aileron", style "Regular", "Version 1.102", designer "Sora Sagano", URL "http://www.dotcolon.net", with no licence entry. Tests: `save::tests::the_sheet_font_is_aileron_and_the_pythons_copy`; the shared fixture `crates/engine/tests/fixtures/sheet.json` (new SHA-256), matched by both SDKs
- Issue: none

### B98. A Save Result path came back with `/` between its folders on Windows, inside the start folder made relative first

- Reported: 2026-11-22
- Report: found by the suite's case "ports: Save Result writes a result node's value in each of its formats" against the Windows app (`native/n5-save`): the file was written where chosen, but `ui.inspect`'s `save.last.paths[0]`, what the person reads, was `C:\…\n5-scratch\conform-scratch/case-18/plain.txt` (the chosen folder inside the folder the app started in) or `C:/Users/…/plain.txt` (outside it)
- Context: `invoke.rs` passed the chosen path through `paths::chosen_path`, the rule for a path input a workflow stores (relative to the workflow's folder, `/` between folders so the workflow opens on any platform). A save is not stored in the workflow.
- Fixed: 2026-11-22, branch `native/n5-save`: a Save Result's path stays as chosen (`save_choice`: only the macOS save panel's `:` becomes folders), with a unit test of a Windows-style path inside a folder and outside it. The case then passed against the Windows app with its scratch folder inside the app's start folder. Other chosen paths read back were checked by reading: Save As of a workflow and Export Bundle pass the chosen path as it is; a viewer's Save As goes through Save Result (fixed with it); Drag Out writes a file the core names; path inputs keep the stored form (`/`) on purpose, as the workflow stores them.
- Issue: none

### B97. Some core tests fail on Windows: Unix-style absolute paths, and engine programs a test expects beside it

- Reported: 2026-11-22
- Report: found running the touched crates' tests on the Windows test VM for B95 (`native/n5-gate-3`); the same tests fail with B95's change taken out, so they were failing before it
- Context: `plumcraft-host` `manifest::tests::a_uv_project_folder_with_no_pyproject_warns` and `a_uv_run_with_no_project_from_a_folder_with_none_warns` (`/nowhere/at/all` and `/elsewhere` are not absolute on Windows, so the rule judges them); `plumcraft-engine` `register::tests::register_from_uvs_cache_warns_and_from_a_temporary_one_refuses` (register.rs:311); `plumcraft-editor-core` `engines::tests::a_desktop_offers_every_hosting_as_before` (engines.rs:964, the engine programs aren't beside a debug test), `switchboard::tests::an_engine_that_exits_at_once_fails_the_switch_at_once_saying_why` (switchboard.rs:950) and `switching_adding_and_clearing_without_a_window` (switchboard.rs:818). CI is Linux only, so nothing ran these on Windows.
- Fixed: 2026-11-22, branch `native/n5-save` (the maintainer: "7A"), at source: `plumcraft-host`'s rule now treats a path with a root but no drive (`\elsewhere` on Windows) as not the manifest's folder, as it treats an absolute one (`from_here`, `crates/host/src/manifest.rs`); `register`'s test reads the written manifest as TOML (its `\` are escaped in the file); `a_desktop_offers_every_hosting_as_before` expects a built-in engine's process hosting disabled exactly when this build hasn't made its program; `switching_adding_and_clearing_without_a_window` adds the text engine's process hosting as a choice before refusing it; the exit-at-once test compares the engine's printed folder with the path as shown (`programs::shown`). All six pass on the Windows test VM; on the Mac, clippy and nextest of editor-core, host, engine and proto pass (396).
- Issue: none

### B91. The Windows app offers Text and Array as a separate process but ships no engine program, so the engine never connects

- Reported: 2026-11-21
- Report: found by the person check (`native/windows/person-check.ps1`) in the N5 gate (`native/n5-gate`): Pick Engines Automatically turned off in Settings, then Engine › Text › Rust · process (mode 2), as a person would
- Context: the core starts a separate-process engine from the program beside the app (`engine_binary`: `plumcraft-text-engine.exe` beside `Plummer.exe`). The Mac app's build copies the engine programs into its bundle (`PLUMCRAFT_ENGINES_DIR`, `project.yml`); the Windows app's build copied only `plumcraft_ffi.dll`, so the core offered the hosting, the person chose it, and the status line stayed at "process (mode 2), not connected". No suite case or test chose a separate-process hosting in the launched Windows app: every one used the in-process engines or the port.
- Verified 2026-11-22 on the Windows test VM: the person check's three runs chose Text › Rust · process (mode 2) and the status line said "mode 2 · process · connected"; in the third, a node ran on it to "HELLO PERSON". The core now disables a hosting whose program is missing, with the reason (another agent's change, merged).
- Fixed in part: 2026-11-21, branch `native/n5-gate`: the app's build copies `plumcraft-text-engine.exe` and `plumcraft-array-engine.exe` beside the app when they are built with the core (`PlumCraft.App.csproj`; the README's build step builds them). Left open: the audio engine is optional and not shipped, and the core still offers a hosting whose program isn't there (it could disable it with the reason, as it does a Python hosting with no Python).
- Fixed: merged into `feat/native-n0` in PR #158 (`native/n5-gate`, 2026-11-22), with the core's half (a hosting whose program is missing is disabled with the reason, `native/n4-decided-2`); verified by a connected run on the Windows test VM (above). The optional audio engine's program is still not shipped; the core shows its process hosting disabled with the reason.
- Issue: none

### B90. The Windows app's Engine menu listed no engines

- Reported: 2026-11-21
- Report: found by the person check (`native/windows/person-check.ps1`) in the N5 gate (`native/n5-gate`), the first launch driven as a person would: the Engine menu showed Reconnect, Attach…, Reload Engine List, Clear Engines, Install Engine…, Cancel Install, then one item with no label and an arrow, whose submenu held the engines
- Context: the core puts the engines in an inline section (`look: "section"`, `submenu: false`, no label: `menu::engine_items`), and the Windows menus drew any item with items as a submenu (`MenuItem.IsSubmenu`). The suite's case "an engine a manifest in the engines folder names is offered" passed against the app because it reads the core's menu model over the port, not what the app draws. Reading the Mac app's `MainMenu.fill`, it draws such an item the same way (an unlabelled submenu); not run on the Mac here.
- Fixed: 2026-11-21, branch `native/n5-gate`: a section's items are put in place (`Menus.MakeAll`, under its label when it has one). Updating the menus in place also paired items with their views by position in an order that put a submenu after its items and counted separators the views don't have; it now uses the order the views are made in (`Menus.Walk`). The person check's Engine menu step lists the engines, the conformance probe from the scratch manifest among them.
- Merged into `feat/native-n0` in PR #158 (`native/n5-gate`, 2026-11-22).
### B95. The copied-manifest warning shows a Windows path with its `\\?\` prefix

- Reported: 2026-11-22
- Report: seen in the engine cell's tooltip on the Windows test VM (`native/n5-gate-2`): "uv run --project . names \\?\C:\…\engines, with no pyproject.toml…"
- Context: the core canonicalises the manifest's folder (`std::fs::canonicalize`, which on Windows gives a verbatim path) and put it in the words a person reads.
- Fixed: 2026-11-22, branch `native/n5-gate-3` (the maintainer: "yes"): `plumcraft_proto::programs::shown` (and `shown_str`) take the prefix off a path for display (`\\?\C:\x` → `C:\x`, `\\?\UNC\server\share` → `\\server\share`, anything else unchanged), used in the copied-manifest warnings (`crates/host/src/manifest.rs`), a duplicate engine id's problem (`crates/editor-core/src/engines.rs`, the one `problems.push` for an id already loaded) and `register`'s uv-cache warning and refusal (`crates/engine/src/register.rs`); paths used for file operations keep it. Tests: string cases on every platform, a canonical temporary folder on Windows, and the copied-manifest warning on Windows names the folder without the prefix. Seen in the app: the engine cell's tooltip names `C:\…\engines`.
- Issue: none

### B117. With the screen locked the person check failed its accessibility steps and left the save panel open

- Reported: 2026-11-22
- Report: found by a person check run at night (branch `native/n4-engine-half`)
- Context: with the screen locked (`IOConsoleLocked`), accessibility sees no app's windows: the check's Recipe, Play, the sheets' layout from outside and the panels' own fields all failed, Cancel couldn't be pressed in the save panel, so it stayed open, File › Close found it, and Quit didn't end the app (killed by its PID). Nothing was pressed; the home listing and the person's Library were unchanged.
- Fix: the check reads `IOConsoleLocked`; while locked, each step that needs accessibility says so (a SEE line) instead of failing, and the panels are cancelled from inside the app (the `panel-cancel` step), which also runs after every panel whether or not accessibility pressed Cancel. Never Save or Open.
- Verified: a run with the screen locked: 48 PASS, 0 FAIL, 6 SEE, the app quit by itself; runs with it unlocked do every accessibility step.
- Seen later (2026-11-22, the gate runs): with the screen locked the app also takes its steps more slowly (probably the system napping a window nobody can see), so the engine half's 3-second slow node finished before Run › Cancel Run was picked (F36), before the second Run (F29), and Run › Auto-Run's check was read before the menu had followed the core (F30). The slow node now takes 6 s and the Auto-Run check is waited for. Runs on the unlocked screen never failed these.
- Fixed: 2026-11-22
- Issue: none

### B116. The Python SDK's executor passed no choice sub-port value and no variadic slot to a node

- Reported: 2026-11-22
- Report: found while fixing B115 (branch `native/n4-engine-half`)
- Context: `python/src/plumcraft_engine/exec.py` `_resolve_inputs` read only a definition's own inputs: a choice's sub-port (`how.width`) and a variadic input's slots (`c1`, `c2`, …) never reached the node, and a sub-port's value never keyed the memo.
- Fix: both resolved as the Rust SDK does (`_is_slot` mirrors its rule).
- Verified: `python/tests/test_subports.py` (5 tests).
- Fixed: 2026-11-22
- Issue: none

### B115. The Rust SDK's executor passed no choice sub-port value to a node

- Reported: 2026-11-22
- Report: found by the ports test engine's conformance case on the egui editor (branch `native/n4-engine-half`)
- Context: `crates/engine/src/exec.rs` `resolve_inputs`: a choice with `options` showed its sub-port on the face (`how.width`) and the graph kept its value, but the node never received it (it ran with its own default), and the value didn't key the memo, so changing it reran nothing.
- Fix: the chosen option's ports arrive as `<port>.<sub>`, wired, from their values or defaults, and key the memo as inputs do.
- Verified: the conformance case "ports: a variadic input, a choice's sub-port…" (egui, the launched Mac app).
- Fixed: 2026-11-22
### B94. The Windows palette's kinds had no names for UI Automation

- Reported: 2026-11-22
- Report: found by the person check (`native/windows/person-check.ps1`, branch `native/n5-gate`): searching the palette for "Text" left one list item named "Text" (the family's heading) and every kind's row unnamed, so the check, or a screen reader, could not tell the kinds apart
- Context: each kind's row is a `Grid` of text blocks, which UI Automation shows as a list item with no name.
- Fixed: 2026-11-22, branch `native/n5-gate`: each row is named by its label and note (`Editors.cs`), and a family's heading is marked a heading. The person check's third run on 2026-11-22 picked "Text" from the palette by its name and passed every step it covers but one (its own reading of a field's value, since fixed).
- Issue: none

### B93. A graph opened from the Windows app's command line is titled Untitled

- Reported: 2026-11-21
- Report: found setting up the sound for the maintainer to hear (`native/n5-gate`): `Plummer.exe --root <scratch> <graph>.json` showed the graph, and the window's title, its status file's `title` and UI Automation all said "Untitled - Plummer"; the same graph opened by File › Open is titled by its file
- Context: the window titles itself from the document's state in the feed (`DocumentState.Name`, else `Path`). The core announces a document's state when it opens it; the command line's graph is opened (`document.open {path}`) before its window's model follows the feed, so that announcement was missed and the window waited for the next. The Mac app titles its windows from NSDocument's file; egui from the core's title; the core's state was right.
- Fixed: 2026-11-22, branch `native/n5-gate-2`: the model reads the document's state once from `ui.inspect` when it starts, as it already read the engine's (`DocumentModel.RefreshDocument`); a kit test (`AGraphOpenedBeforeItsModelIsKnownByItsFile`) and a person-check step (the saved graph opened from the command line is titled by its file) pin it. The person check passed it three times.
- Issue: none

### B110. A node working on an engine that is killed mid-node kept its progress on its face for good

- Reported: 2026-11-21
- Report: found by the person check's engine half (decision 10, branch `native/n4-decided-2`): the installed engine's process killed while `text.slow` was at 40%
- Context: `crates/session/src/run.rs` `apply_reply`. With more than one engine in the document (the built-in text engine and the installed one), the coordinator answered the run with the engines that were still there and the run's error; the slow node had no final record and was in the reply's nodes, so its state stayed `progress` (its fraction too) after the run ended: the face showed a node still working that nothing would ever finish. `RunState::fail` (the document's only engine gone) unmarks such nodes, but it is not called while another engine is connected.
- Fix: when a run's reply comes, a node still marked working (started, progress, preview) in a failed run is in error with the run's error (`engine` kind: "the engine stopped (signal 9); restarting"), and one only queued is unmarked, whatever the reply says of it.
- Verified: `run::not_run_tests::a_node_still_working_when_a_failed_run_ends_is_in_error`; the person check's F35 step on the launched Mac app (the face goes from progress to error, "engine error: the engine stopped (signal 9); restarting", the run ends 1.6 s after the kill, the engine comes back).
- Fixed: 2026-11-21
- Issue: none

### B87. An engine manifest the core can't read is skipped without a word to the person

- Reported: 2026-11-21
- Report: found by the person check (branch `native/n4-person-gate`)
- Context: a manifest with an id the core refuses was left out of the Engine menu silently at start; only Engine › Reload Engine List's reply carried `problems`, and the Mac shows no reply.
- Fix (decision 11, "fix B86 and B87 now"): what the last read of the manifests couldn't use or warned about is kept with the engines (`engine.list`'s and `ui.inspect`'s `engine.manifestProblems`, one line a manifest), put in the status line of a document opened after the read once its startup has settled (`engine manifests: …`, or `engine manifests: N problems; the first: …`), and in the log as before; Reload Engine List updates both (`Document::note_manifest_problems`). `docs/interface.md` 16.6.
- Verified: `engines::tests::an_unreadable_manifest_is_reported_to_the_person`, `engines_install::an_unreadable_manifest_is_said_to_the_first_document` (C ABI: the feed's document state and `ui.inspect`); the person check: a manifest with a hyphenated id in the scratch engines folder is in `manifestProblems` and the Mac window's banner at launch.
- Fixed: 2026-11-21
- Issue: none

### B86. A node's Run on menu names engines by their ids, not their names

- Reported: 2026-11-21
- Report: found by the person check (branch `native/n4-person-gate`)
- Context: `crates/editor-core/src/menu.rs`, the node context menu's Run on: an installed engine was offered as `person_text`, the built-in one as `text`.
- Fix: Run on names each engine as the Engine menu does (its choice's label; an engine with no choice keeps its id); the command and its params are unchanged (`menu::Parts` carries the engine choices; egui's canvas passes them).
- Verified: `menu::tests::run_on_names_engines_by_their_labels`; the person check picks Run on › Person Check Text on both nodes of the launched Mac app.
- Fixed: 2026-11-21
- Issue: none

### B84. A scripted launch that shows the Mac's Save or Open panel changes the person's defaults, or saves into their folders

- Reported: 2026-11-21
- Report: found by the person check's first runs (branch `native/n4-person-gate`)
- Context: AppKit's panels run out of process and keep their folder in the app's defaults domain, which can't be pointed elsewhere (B50).
- Fix (decision 7, "scripted launches get their own bundle id"): `native/apple/PlumCraftMac/scripted-app.sh` makes a copy of the built app under `com.swapwizard.plummer.mac.scripted` (a clone, signed again ad hoc for this Mac only) and, when the run ends, removes what the system kept for that id (defaults, saved state, recent documents, caches, HTTP storage, WebKit, the temporary folder's state) and unregisters and removes the copy; the app refuses `--scratch-state` under any id that is not a scripted copy's (`ScratchGuard.bundleRefusal`); `conformance.sh`, `person-check.sh` and the Mac `bench.sh` run the copy, and every one lists the person's home top level, `~/workspace` top level, `~/Documents` and `~/Desktop` before and after and fails loudly when anything came or went (`library-check.sh home`), besides the person's Library for both their ids. The panels are set before they show (`GraphDocument.prepareSavePanel`, the core's file asks), read from outside through accessibility (the panel's own name field and folder) and cancelled: **no real Save or Open is pressed** (the maintainer, 2026-11-21: "1,2 as you say"); the press is behind `PERSON_PRESS_SAVE=1`, which no script sets and `person-check.sh` refuses, and `may_press` (tested, 12 cases, each run) refuses unless both fields are a scratch folder's.
- Found on the way: the scripted copy's first save through the real panel, set after the panel showed, landed in the person's `~/Documents` as `Untitled.json` (the run's home listing caught it; deleted by the controller); that is why nothing is pressed. And the scripted copy, noting a recent document, stopped in Swift's isolation check: `PlummerDocumentController.maximumRecentDocumentCount` is read by AppKit from a queue of its own; it is `nonisolated` now, and the person check checks the app is alive after a recent document is noted.
- Verified: the person check, three runs in the Mac gate: the person's build refuses a scripted launch; the save panel opens on the scratch folder with the name (from the panel itself); the open panel opens on the scratch folder; the person's Library for both ids, the home listings and the engines folder unchanged; the scripted id's Library as before after the cleanup.
- Fixed: 2026-11-21
- Issue: none

### B83. The control channel acts on the first document only, and ignores a request's `doc` without saying so

- Reported: 2026-11-21
- Report: found by the person check (branch `native/n4-person-gate`)
- Fix (decision 8, "the control channel honours the document a request names"): the control server passes a request's top-level `doc` on (`ControlRequest.doc`); the C ABI core answers for that document (the first with none), and refuses one that isn't open (`code: "unknown_doc"`) or isn't a number (`bad_params`); the egui editor, which serves one document, refuses any `doc` (`unknown_doc`). `docs/control-protocol.md` and `docs/shell-interface.md` 4.1. No conformance case: a driver over the port can't open a second document.
- Verified: `server::tests::auth_first_then_requests_reach_the_ui`, `control_port::the_control_port_honours_the_document_a_request_names`, egui `control::a_request_naming_a_document_is_refused_not_answered_for_this_one`; the person check's engine half reads its own window's scene by `doc`.
- Fixed: 2026-11-21
- Issue: none

### B81. Opening File › Open Recent in a scripted launch of a newly built Mac app rewrites the person's recent-documents list

- Reported: 2026-11-21
- Fix: falls out of B84 (decision 9, "leave Open Recent empty in scripted launches without touching the person's list"): a scripted launch is the scripted copy, whose recent documents are its own list (it starts empty, the script removes it at the end).
- Verified: the person check: the saved graph is in the copy's own recent list and opened from it; the person's `com.swapwizard.plummer.mac.sfl4` byte for byte unchanged (`library-check.sh`, size, time and hash). Not seen: Open Recent's menu items, which AppKit fills only while the menu tracks (the check never takes the pointer), so the check opens the list's entry as the item does (a SEE line).
- Fixed: 2026-11-21
- Issue: none

### B76. The iPad bench leaves each Instruments trace's working file in the temporary folder

- Reported: 2026-11-21
- Cause: the raw kernel trace (`instruments*.ktrace`) is written by Xcode's `DTServiceHub`, a service launchd starts, into the per-user temporary folder (`getconf DARWIN_USER_TEMP_DIR`), never into the TMPDIR given to xctrace, and is left there (still open in DTServiceHub, which lingers). Shown without a device: a three-second System Trace of `/bin/sleep` with TMPDIR set left a 386 MB `instruments….ktrace` there and nothing in xctrace's folder (`claude-debug/n4-decided-2/b76-probe.sh`).
- Fix: `native/apple/ktrace-sweep.sh` lists the raw traces there before a recording and removes every new one once its tables are exported, or on any way out (the EXIT trap); `PlumCraftPad/bench.sh` and `PlumCraftMac/bench.sh` use it.
- Verified: on the Mac, a recording with the sweep around it as the scripts use it: one new 257 MB raw trace, removed, none left (`claude-debug/n4-decided-2/b76-verify.sh`). Not verified: a device trace (no device use allowed); the space of a removed file comes back when DTServiceHub closes it.
- Fixed: 2026-11-21
- Issue: none

### B56. The sound case races a shell that reports a sound's end: under emulation the 64 ms sound ends between two steps

- Reported: 2026-11-21
- Fix (decision 15): the case sets `loop` before `play`, so no shell reports the sound's end while the case runs, however slow (the Apple apps' player and muted clock report no end while looping); the case still checks play, seek, level, loop and pause.
- Verified: the core's document, the C ABI and the egui editor (`cargo nextest`, the conformance tests), the Mac app (`conformance.sh`, 57 of 57). Not run: Windows under emulation.
- Fixed: 2026-11-21
- Issue: none

### B80. The Apple apps' Engine menu put every engine in a submenu with no title

- Reported: 2026-11-21
- Report: found by the person check, the floor re-walk's first launch as a person (branch `native/n4-person-gate`)
- Context: the core's Engine menu ends with a section (`look: "section"`, not a submenu, no label) that holds every engine (Text, Audio, Array, the craft engines, the installed ones). The kit's `MenuItem.isSubmenu` counted any item with items as a submenu, so the Mac drew the engines under a blank item at the foot of the Engine menu, and the iPad's menu bar and More menu did the same. A person saw Reconnect, Attach…, Reload Engine List, Clear Engines, Install Engine… and Cancel Install, and one unlabelled arrow: the engines, installed ones included, were there only to someone who opened it. Part of what B71's "no engines available" saw.
- Fix: `MenuItem.isSection`; a section's items are drawn inline in the menu they are in, under a heading when it has a label (Mac menu bar and context menus, the iPad's menu bar and More menu).
- Verified: the person check lists the Engine menu as AppKit validated it: every engine by name at the menu's level, the installed one with them, no untitled item (`native/apple/PlumCraftMac/person-check.sh`). The iPad's drawing was built for the simulator and a device (iOS Release) but not looked at.
- Fixed: 2026-11-21
- Issue: none

### B82. The Mac's Install Engine… form cut its hint off

- Reported: 2026-11-21
- Report: the person check's picture of the Install sheet (branch `native/n4-person-gate`)
- Context: the repository field's hint, "owner/repo or https://github.com/owner/repo", was wider than the field (280 points), so a person read "…github.com/owner/".
- Fix: a form's fields are as wide as their longest hint needs, up to 440 points, and every field carries its whole hint as its tooltip (`native/apple/PlumCraftMac/App/Asks.swift`).
- Verified: the person check's picture of the sheet (`claude-debug/native-n4-person-gate.md`).
- Fixed: 2026-11-21
- Issue: none

### B72. Starting a manifest engine as a process that exits at once takes 30 s to fail

- Reported: 2026-11-21
- Report: found by the controller checking B71: `engine.switch` to an engine whose program exits immediately (its Python module is not installed) got no reply for 30 s as a process; over HTTP the same engine failed in 0.2 s with "the engine exited before listening".
- Context: `crates/editor-core`, the process start of an engine a manifest names.
- Cause: the process host (`crates/host/src/process.rs`) saw the child exit before it connected within milliseconds, but treated it as a crash: it said `engine_down` and restarted it, five times in about 3 s, then stopped. `ProcessHost::wait_ready` kept waiting while the engine was down whether or not anything would restart it, so the editor core's start (`connect_external`, and `start` for the built-in engines' process mode) waited out its whole limit (60 s for a manifest's engine, uv may build an environment first; 10 s for the text and audio binaries), longer than the control channel's 30 s, and then made a link to a dead engine: the switch "worked". The child's stderr was kept, but on its own thread with nothing waiting for its last lines. Over HTTP the host already failed at once, but its stderr was inherited, so the reason had no last words.
- Fix: 2026-11-21, branch `fix/b72-b37-b38`. An engine that exits (or can't be started) before it has ever connected is not restarted: the host gives up at once, with a reason that says how it exited and ends with the last five lines of its stderr (after waiting up to 1 s for the pipe to close), and `ProcessHost::wait_started` returns that reason as an error. `wait_ready` no longer waits for an engine supervision has given up on (this one, or one that crashed five times in a minute). `hello`, `session.confirm` and `schema` during a start give up as soon as the child exits, not after 10 s. One the host stops for not connecting within 10 s is restarted as before. The editor core's starts (a manifest's engine, the built-in engines and packs in process mode) and the CLI use `wait_started`, so `engine.switch` fails with the reason. An HTTP engine's stderr is now copied through as before and its end kept, so its "exited before listening" carries the same last words. `docs/interface.md` 16.3 says so.
- The folder question: `docs/interface.md` 16.6 already says `cwd` is relative to the manifest's real folder (a link is followed back to the project), and `plumcraft_host::manifest::load` does that, so it is not a second bug. The installed OpenCV manifest (a link into its project) started fine from a folder that wasn't its project, as a process and over HTTP; a copy of it in another folder gave exactly the report's "No module named opencv_engine", in both hostings (`claude-debug/b72/opencv_cwd.sh`). The report's manifest was very likely a copy, not the link. `--project .` is uv's own argument and resolves against that `cwd`.
- Follow-up: 2026-11-21, branch `fix/manifest-warn-python-clock` (the maintainer: "1,2,3 yes"): such a copy now loads with a warning, and its failed start ends with what to do about it (`docs/interface.md` 16.6).
- Verified: new tests, a manifest in a scratch folder whose command prints its folder and "No module named opencv_engine" to stderr and exits 1: `engine.switch` as a process failed in 10 ms and over HTTP in 26 ms, each with the exit status, the last words and the manifest's folder (`switchboard::tests::an_engine_that_exits_at_once_fails_the_switch_at_once_saying_why`). The same test against the old code waited 60 s and then the switch succeeded with a dead engine. A text or audio engine binary that exits at once fails the start within 3 s in both hostings (`engines::tests::a_built_in_engine_binary_that_exits_at_once_fails_the_start_at_once`). `mode2_an_engine_that_cannot_start_is_down_not_hung` now asserts the reason, that the start fails within 2 s and that the program was started once. On the Linux test host all of these passed 60 of 60 (30 quiet, 30 under load). Not checked on Windows (its variant of the switch test uses `cmd`).
- Fixed: 2026-11-21
- Issue: none

### B37. `fast_never_session_too_big_and_setup_bound_results_are_not_persisted` failed once on the Linux test host

- Reported: 2026-11-20
- Report: "record bugs"
- Context: `plumcraft-engine::persist`. Failed once in a full parallel run while three other builds were running on the host; the message was not captured. Passed 3 of 3 alone. Not looked into.
- Seen again: 2026-11-20, in the full run for N3 slice 2 with nothing else building. It fails at `crates/engine/tests/persist.rs:166`, the "too big" case: a 60 ms node that makes 32 MB was persisted, where the test expects it not to be worth it. Passed 40 of 40 alone. The test's own comment says the margin is a matter of timing.
- Seen again: 2026-11-20, on the Mac in N4 slice 2's gate, in the Python suite's test of the same name (`python/tests/test_persist.py`) while cargo was running in parallel. Passed 10 of 10 alone.
- Seen again: 2026-11-20, on the Linux test host in N4 slice 2's full run (the Rust test), together with B38. Both passed 10 of 10 alone afterwards.
- Note: 2026-11-21, B49 found the Python test's cause: the work is measured on the wall clock, and allocating the big array can stall for over 150 ms while the kernel compacts memory, which makes a 60 ms node look worth persisting. The Rust test probably fails the same way; it has no virtual clock to pin the work, so its fix needs an injectable clock in the Rust executor or a much bigger margin. Not checked.
- Cause: as B49's. The test measures a node's work on the wall clock, and the "too big" case (about 32.18 MB of outputs, worth persisting after 321.8 ms of work) gave the rule 262 ms of room over a 60 ms node; when allocating the 32 MB array stalls past that on a busy host, the 60 ms node rightly looks worth persisting. Not reproduced on the Linux test host by load alone: under B49's load (32 niced spinners and 6 memory churners, load average 30 to 43, about 220,000 compaction stalls system-wide over the runs) the old test passed 30 of 30, and 30 of 30 quiet. The host's transparent huge pages are `madvise` only and Rust's 32 MB `malloc` never asks for them, so the stall that hit numpy doesn't hit this allocation in the same way; what stalled it in the full gate runs (the gate's own builds and 1,500 tests in parallel) wasn't captured. The rule stays as it is (the maintainer: "Leave the rule").
- Fix: 2026-11-21, branch `fix/b72-b37-b38`, the smallest SDK change: `plumcraft_engine::clock` (a `Clock` trait, `now` and `sleep`; `RealClock`, the default; `VirtualClock` for tests) and `Engine::with_clock`; the executor measures a node's work on the engine's clock. No behaviour change in production. The persist tests' node sleeps on the clock it is given, and the not-persisted cases run on a virtual clock, on which a node's work is exactly the time it sleeps. A new test, `too_big_is_weighed_against_the_work_measured`, pins the rule either side of its limit: 32.18 MB after 300 ms of work is not persisted, after 345 ms it is, and a node that sleeps no time on a virtual clock did no work, whatever its allocation took.
- Verified: on the Linux test host with a preloaded shim that makes every `malloc` of 16 MB or more wait 300 ms (`claude-debug/b72/stall.c`, `linux_stall.sh`): the old test failed 30 of 30, every time at `persist.rs:166` ("too big"), and the new file passed 30 of 30. Without the shim, old and new both passed 30 of 30 quiet and 30 of 30 under the load above. On the Mac, the engine crate's 72 tests and the full gate.
- Fixed: 2026-11-21
- Issue: none

### B38. `memo_reuses_unchanged_nodes` failed once on the Linux test host

- Reported: 2026-11-20
- Report: "record bugs"
- Context: `plumcraft-text::conformance mode2_process`, `crates/text/tests/conformance.rs:390`, "off really runs the slow node". Failed once in a full parallel run while three other builds were running on the host. Not looked into.
- Seen again: 2026-11-20, on the Linux test host in N4 slice 2's full run, together with B37. Passed 10 of 10 alone afterwards.
- Cause: a timing race in the test, not a persist decision and not the process engine. The case ends by running the graph with `memo: off` and asserting that it took at least 300 ms, the slow node's sleep. Its stopwatch started after `run` returned, that is after the engine's acceptance had come back, but the engine starts the graph before that acceptance reaches the test. Any lateness in the acceptance (a busy host, where the reply waits for a thread to be scheduled) was taken off the 300 ms, so a run that really slept 300 ms could measure less. Every assertion before it (the states, the plan keys) had passed: the node did run.
- Fix: 2026-11-21, branch `fix/b72-b37-b38`: the stopwatch starts before the run is sent, so a reply can't come back sooner than the slow node's own 300 ms. No retry, no wider margin.
- Verified: on the Mac, with a 60 ms sleep put between the acceptance and the stopwatch (a late acceptance), the old timing failed in all six hostings (in-process, coordinated, process, Python process, HTTP, Python HTTP) at exactly "off really runs the slow node", and the new timing passed in all six with the same sleep. Not reproduced naturally: on the Linux test host the old test passed 30 of 30 quiet and 30 of 30 under load, as did the new. What would settle it further: a failure of the new test, which would mean the slow node returned early.
- Fixed: 2026-11-21
### B53. The C# conformance runner passed a `holds` step on its first reply alone

- Reported: 2026-11-21
- Report: found reading the runner against `crates/editor-core/src/conformance.rs` in N5 slice 4 (`native/n5-panels`)
- Context: the suite's `holds: {pointer: want}` step form (with `for`, added for B43: a window the shell closed by itself a moment after it opened) asks again for that long and every reply must match. The Swift and Rust runners do; the C# runner, ported before the form existed, ignored `holds` and passed such a step on its first reply, so "the shell never closes what the core opened" was not checked by the C# runner against the Windows app in slices 2 and 3.
- Fixed: 2026-11-21, branch `native/n5-panels`: `holds` and `for` as the other runners have them (`native/windows/src/PlumCraft.Conformance/Runner.cs`). The Windows app passes the three such steps.
- Issue: none

### B73. The Mac app's forms put every label above the wrong field, the first one clipped

- Reported: 2026-11-21
- Report: "The attach dialog is meh", with a picture: "Address" cut off at the top, "Token" above the address field, the token field unlabelled.
- Cause: `Asks.swift` added all the labels to a stack, then all the fields, in a frame sized for the fields alone. Every form the core asks the Mac for (Attach, Install) was drawn this way. Nothing checked what a form looks like: a scripted ask shows no sheet.
- Fix: a form is a grid, each label right-aligned beside its field on its baseline, sized to fit, with the fields in tab order.
- Verified: the same grid in a system alert drawn to a picture (`claude-debug/b73/form.swift`), labels beside their fields; Mac conformance still passes. Not seen in the app's own sheet by the controller.
- Fixed: 2026-11-21
- Issue: none

### B71. The native apps can't install an engine and don't offer the engines installed

- Reported: 2026-11-21
- Report: the maintainer, on first using the Mac app (Plummer) by hand: "Install engine is greyed out, no engines available. WTF?" They had been told twice that the Mac build could install an engine; it could not.
- Context: Engine › Install Engine… was disabled in Plummer with "this editor can't install engines", and the Engine menu offered none of the engines in `~/.config/plumcraft/engines` (DuckDB and OpenCV, installed with the egui editor). Below the floor: the egui editor installs and offers them.
- Cause: the installer, the manifest reader and the decision to offer installing were in the editor core already (since N1 slice 2), but only the egui application wired them: `apps/plumcraft-editor` called `load_manifests` at start, gave the document a Reloader and an Installer, and `EditorApp::with_installer` set `can_install`; the egui shell's own frame loop followed the install (`poll_install`). The C ABI's core did none of these: it never read the engines folder, had no Reloader (so Reload Engine List did nothing but say "this editor has no engine manifests to read"), no Installer, and never set `can_install`, whatever the platform declared. A test asserted the refusal (`attach_asks_with_the_cores_form_and_install_is_refused_here`). So whether a shell could install was decided by which shell it was, not by the platform: a breach of "the shell decides nothing".
- Fixed: 2026-11-21, branch `fix/b71-native-engines`: the core wires it for every shell. `Launcher` gains `folder` (the engines folder: `dirs.engines`, else `$PLUMCRAFT_ENGINE_FOLDER`, else `~/.config/plumcraft/engines`, the folder every editor shares) and `uv`; `Launcher::load_manifests`, `reloader` and `install_setup`; `Document::offer_installs` gives a document the Reloader and, where `install::refusal(caps)` allows (the platform declares `subprocess` and `python`), the installer, else disables Install Engine… with the reason ("This device can't install engines: it can't start programs" on the iPad). The C ABI's core reads the manifests on its thread at start (`engines.manifests` and the folder) and offers each document what any document read again (`Document::follow_manifests`). `control::step` follows the install, in every shell. The installer is whole in the core (`install::install_engine`): uv found as a manifest's is (the login shell's PATH and the usual folders, since an app from the Finder has none; a miss says where it looked), `register --folder <engines folder>`, each step in a process group of its own with the search PATH and a 30-minute limit, what it is doing (with uv's latest line) as `engine.installing`/`installStatus` and the `document` domain's `installing`, and `engine.install.cancel` (Engine › Cancel Install), new, which stops it and what it started. The egui shell lost its own Reloader and install loop (`EditorApp::with_launcher` takes the core's). Plummer shows "Installing an engine…" in its engine cell while one runs. Verified by: core tests with a stand-in uv (install, register into the folder given, uv's failure, cancel within 5 s, a step past its limit, uv missing), the C ABI tests (a desktop platform's manifests offered from the start, Install Engine…'s form, an install, a second document offered it, a cancelled install; the iPad's platform refused with the reason and an attached manifest engine still offered; bad config fields refused), two new conformance cases on every driver (the core's document, the C ABI and its port, egui, the Swift kit and its port, the Mac app: 55 of 55), and a real install through an ad hoc signed, hardened Plummer launched as the Finder would (a bare environment), on scratch: `seantrue/plumcraft-duckdb-engine` installed and registered in 7.7 s, started as a process (a Homebrew Python, a separate process, so library validation doesn't apply), and a SQL Query node gave 42 (`claude-debug/native-engines-install.md`).
### B70. The Mac app could not make a new graph: File › New did nothing, and a launch showed only the Open panel

- Reported: 2026-11-21
- Report: "I can't create a new graph? Only open an old one?"
- Context: the maintainer's first launch of the Developer ID build of the dev branch. With no file to open the app showed File › Open's panel, with no way to make a new graph.
- Cause: the app declared its document type with the role Viewer. AppKit makes a new document only of a type the app can edit, so `newDocument` did nothing at all: File › New and its key were dead, in every build since the Mac app became document-based. No test or script noticed, because every scripted launch opens a file, and nobody had launched it by hand without one. The controller first told the maintainer that New worked once the panel was cancelled, from reading the code; that was wrong.
- Fix: the document type's role is Editor (`native/apple/PlumCraftMac/project.yml`). Asked whether a launch should open an untitled graph or the panel should gain a button, the maintainer chose "1": a launch with nothing to open, and a Dock click with no graph open, make a new untitled graph (`AppMain.swift`).
- Verified: a scratch launch with nothing to open reported no window before the role change and one ready, untitled, unedited graph after it; the Mac conformance suite 53 of 53 with the person's Library unchanged. Not verified by a person, and no automated check launches without a file yet.
- Fixed: 2026-11-21
### B49. `test_fast_never_session_too_big_and_setup_bound_results_are_not_persisted` fails on the Linux test host while it is busy

- Reported: 2026-11-21
- Report: found by the Linux gate for the egui parity branch; recorded under the standing "record bugs"
- Context: `python/tests/test_persist.py`. It failed in the gate's full Python run (1 failed, 223 passed) and in 4 of 5 runs of the file alone straight afterwards, while the host's load average was above 20; it then passed in 11 runs of the file and one full run (224 passed) once the load fell. The branch changed no Python. The assertion was not captured: the gate keeps only the last two lines of pytest's output.
- Seen again: 2026-11-21, in the gate for the private-strings branch and then in 6 of 6 runs of the file alone, with the host's load average near 19. The assertion, at `tests/test_persist.py:100`: `assert twice(r, {"kind": "t.slow", "values": {"n": 4, "ms": 60, "big": 8_000_000}}) == ("done", 2), "too big"` got `('cached', 1)`: a result meant to be too big to persist was persisted and came back cached. So it is not a timing miss in the test's own waits; whatever decides "too big" gives a different answer while the host is busy. Not looked into further.
- Cause: the test, not the executor. Whether a result is too big is weighed against the node's own work as measured on the wall clock (spec 14.5: its size at ~200 MB/s, twice, under the work time), and the case gave that rule 22 ms of room: about 8.2 MB of outputs costs 81.8 ms to write and read back, and the node sleeps 60 ms and then makes an 8 MB array with `np.full`. numpy asks for huge pages for so large an array (`madvise`, which the host's `transparent_hugepage/defrag` setting honours), so when the host's memory is fragmented (swap full, almost no free blocks of 2 MB, as the gate's builds leave it) the kernel compacts memory while the array's pages fault in. That took 33 to 157 ms on the host, against 0.3 to 0.7 ms otherwise; with numpy's huge-page advice turned off it never took over 7 ms and caused no compaction stalls (`/proc/vmstat compact_stall`), with it on 6 to 9 stalls per first allocation (`claude-debug/b49/probe_thp.py`, `probe_breakdown.py`). The sleep itself was always 60.1 ms. So the work measured was 62.4 to 71.5 ms in passing runs and 87.6, 90.9, 96.8 and 122.8 ms in failing ones (`probe_work_ms.py`), over the 81.8 ms limit, and the rule rightly persisted it. The load average is a proxy: the test failed 2 of 3 runs with the load at 1.6 while memory was still fragmented, and passed 10 of 10 later at a load of 25. The executor does what the contract says: a node that really took 97 ms is worth persisting at 8 MB. Its Rust twin (B37) has a wider margin (32 MB, 320 ms) and probably fails the same way; not checked.
- Fixed: 2026-11-21, branch `fix/b49-persist-too-big`: the tests of what isn't persisted run on a virtual clock (`VirtualClock` in `python/tests/test_persist.py`, the executor's `time` module patched for the test; since `fix/manifest-warn-python-clock`, given to the engine with `Engine.with_clock`, as in Rust), on which a node's work is exactly the time it sleeps, so the fast, setup and too-big cases no longer depend on how fast the machine is. A new test, `test_too_big_is_weighed_against_the_work_measured`, pins the work: 8.2 MB after 60 ms of work is not persisted, after 97 ms (a failing run's value) it is. No SDK code changed. Verified on the Linux test host: with a pytest plugin that adds 40 ms to every `np.full` of 1 MB or more (`claude-debug/b49/b49_stall.py`), the old file failed 3 of 3 and the new one passed 3 of 3, with the host quiet; under a load of 32 niced CPU spinners and 6 processes churning 64 MB to 1 GB arrays (`claude-debug/b49/load.sh`, load average 35 to 53), the old file failed 6 of 10 at the same assertion and the new one passed 30 of 30, and the full Python suite passed 225 of 225; quiet, the old file passed 10 of 10, the new one 30 of 30 and the full suite 225 of 225. `ruff check` clean.
- Issue: none

### B50. A scripted launch of the Mac app writes to the person's real defaults domain

- Reported: 2026-11-21
- Report: found by the before and after check of the decisions branch (`native/decisions-1121`); recorded under the standing "record bugs"
- Context: the Mac app's conformance runs (`--scratch-state`, scratch store, prefs, caches and logs) left `~/Library/Preferences/com.swapwizard.plummer.mac.plist`, the new id's real defaults file, created at the time of the runs; it holds an empty dictionary. The old id's file, `com.plumcraft.mac.plist`, holds keys such as `NSWindow Frame PlumCraft Pop-Out count.count`, the pop-out windows' remembered frames (`Popouts.swift`'s `setFrameAutosaveName`; the Settings window does the same), which look like they came from earlier scripted runs. So `--scratch-state` keeps the core's files in scratch but not AppKit's defaults. Not fixed: a scripted launch could skip frame autosave names, or run in a defaults suite of its own. Not checked: which call created the empty file.
- Cause: the pop-out and Settings windows already skipped their frame autosave names under `--scratch-state`, and saved state was already off (`ApplePersistenceIgnoreState` and `NSQuitAlwaysKeepsWindows` in the argument domain), but every document a scripted launch opened went into the person's recent documents: `NSDocumentController` notes it in the system's shared file list (`~/Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments/com.swapwizard.plummer.mac.sfl4`), which held only the scratch paths of conformance runs (`plumcraft-mac-conformance.*/empty.json`). With AppKit's own recent count, the first launch of each new build also rewrote that list's properties (the code requirement it carries). The empty defaults file was not reproduced: none of the runs below wrote the domain, so which call created it is still not known. AppKit's own panels (the colour panel, Open and Save) keep frames and the last folder in the domain (the old id's file holds such keys), and the domain cannot be pointed elsewhere: `CFFIXED_USER_HOME` and `__CFPREFERENCES_AVOID_DAEMON` both still read the real one. No conformance case opens those panels.
- Fixed: 2026-11-21, branch `fix/b50-scripted-defaults`: the Mac app's document controller (`PlummerDocumentController`, `native/apple/PlumCraftMac/App/AppMain.swift`, made in `main` so it is the shared one) neither notes a recent document nor keeps any (a count of 0) under `--scratch-state`, so AppKit leaves the list alone; a person's launch is AppKit's as before (frames remembered, recents noted). The iPad and iPhone app keep everything in their own container, and a scripted launch there already skips the first-launch flag and scene restoration; not checked on a device. Verified by `native/apple/PlumCraftMac/library-check.sh`, which `conformance.sh` now runs around the app: it snapshots the defaults domain (cfprefsd's answer and the plist's size, time and hash, ByHost too), the saved state (and anything AppKit leaves in its temporary copy), the recent documents list and the app's caches, logs and application support folders before the launch and 10 s after the quit (`SETTLE`; a noted recent landed 3 to 8 s after the quit), and fails the run on any difference. Before the fix the read half failed it (the recent documents list changed); after it, the read half (31 of 31) and the edit half (21 of 21) leave the Library unchanged, as do two launches of new builds. Defect caught on the way: a first try also returned an empty `recentDocumentURLs`, and AppKit wrote that empty list over the person's (which held only the two scratch entries). The empty `com.swapwizard.plummer.mac.plist` is left as it was (42 bytes, 2026-11-21 16:29).
- Issue: none
### B51. The Windows app dropped every mouse click and mouse drag on the canvas

- Reported: 2026-11-21
- Report: found by N5 slice 3's input probe (`native/n5-editing`); recorded under the standing "record bugs"
- Context: slice 2's report said a mouse click injected on the Windows test machine "did not arrive" after the machine restarted, while hover did. With the injector now logging what it can see (its session and integrity level and the app's, both session 1 at High; the foreground window; `GetAsyncKeyState` after each button), the presses and releases did arrive: the app's own status listed `pressed Mouse … L` and `released Mouse …`, and nothing followed. Touch and pen worked.
- Cause: `CanvasView.OnReleased` released the pointer capture before handling the release, and `ReleasePointerCapture` raises `PointerCaptureLost` at once, whose handler took it as a cancel and cleared the press; the release that followed found no press. Every mouse click and drag on the canvas did nothing (right clicks, through `ContextRequested`, still worked).
- Fixed: 2026-11-21, branch `native/n5-editing`: the release is handled first and the capture let go after, and a capture loss the view asked for (`releasing`) is not a cancel. Verified on the test machine with real input through `SendInput`: a click on empty canvas ran `selection.clear`, a drag of a face `edit.move` ("Move Node"), a drag from a pin `wire.connect`, a double click on empty canvas the palette, a click on a row the field editor (`claude-debug/native-n5/r3-probe-run2.txt` and later). Before the fix the same input ran nothing (`r3-probe-run1.txt`).

### B45. In the egui editor, hiding a row from a node's face shifts the rows, and an edit on the face changes the wrong port

- Reported: 2026-11-21
- Report: found by the egui audit the maintainer asked for ("Do the egui audit on [the Linux test host]"); recorded under the standing "record bugs"
- Context: the dev branch at `057ed2d`, on the Linux test host through the kittest harness. Add "Every widget", then `node.face {port: "count", shown: false}`: the labels, fields and pins no longer line up, and dragging the field beside "amount" changed `count` from 3 to 4. The same with Replace's `find` hidden. Graphs saved by the Apple apps can carry this state (Show on Face). Steps and pictures: `claude-debug/egui-audit.md`, defect D1. The audit's other findings in egui (D3 to D8: the palette not offering kinds of engines not started, viewers not following the core's viewer state, no rows reported to the core, no Match the System, double clicks not following the core's rule, a repeated word in the rename window's title) are gaps listed there, not recorded here one by one.
- Cause: egui built each face's rows from `input_rows` less the fields its choice tucks away, while the core's scene (which egui draws labels and pins from) also leaves out the rows `node.face` hides (`rows::shown_rows`). With a row hidden, egui's pin `i` took the scene's row `i` for its label and the catalogue's row `i` for its field, so every row after the hidden one was off by one.
- Fixed: 2026-11-21, branch `native/egui-parity`: egui builds its rows with `shown_rows` (`crates/ui/src/canvas.rs`), so pin `i` is the scene's row `i`; a row whose key isn't the scene's for that pin draws no field (no edit by another port's label). Verified by `crates/ui/tests/kittest_face_rows.rs`: a drag beside "amount" with `count` hidden on Every widget changes `amount` and not `count`; a check box beside "flag" changes `flag`; typing beside "with" with `find` hidden on Replace changes `with` and not `find`; wires into rows before and after the hidden one land on their ports; a graph opened with two rows hidden draws and edits right; and Show on Face in egui's parameters window hides and shows a row. Each failed before the fix (the rows check) and passes after. Pictures: `claude-debug/egui-parity/shots/{before,after}/b45-*.png`.
### B48. `build-xcframework.sh` ignored `CARGO_TARGET_DIR`

- Reported: 2026-11-21
- Report: found while preparing the Apple builds
- Context: the script read the libraries back from the checkout's own `target/` while cargo, honouring `CARGO_TARGET_DIR`, built them elsewhere; in a git worktree pointed at the main checkout's target directory it would have packaged stale libraries, or failed, rather than the ones just built.
- Fixed: 2026-11-21, Apple prep: the script reads from `CARGO_TARGET_DIR` when it is set. Verified by building the xcframework from a worktree into the main checkout's target directory.

### B47. A Mac app built for distribution would carry no engine programs

- Reported: 2026-11-21
- Report: found while preparing the Apple builds (the maintainer's standing instruction to record bugs found on the way)
- Context: the Mac app's Release bundle held only `PlumCraft`. The core looks for engine programs beside the app's own (`Contents/MacOS`), or in the cargo build a test runs from, so on another Mac the audio engine (never in process) and every engine hosted as a process (Engine › Rust · process) would have failed with "no engine binary". In-process text and array, which the suite uses, worked, so nothing caught it.
- Fixed: 2026-11-21, Apple prep (branch `native/n4-apple-prep`): `native/apple/release.sh mac` builds the text, array, audio and craft engines for both Macs and the Mac target's Bundle engines phase puts them in `Contents/MacOS`, each signed with the hardened runtime before the app seals them. Verified on the exported Developer ID build: the text engine switched to a process, audio and array added as processes, a graph run, the three children running from the bundle with the runtime flag. A plain Xcode build still has none (it runs engines from the cargo build, as before).

### B44. The iPad UI tests fail on a second run in the same app container

- Reported: 2026-11-21
- Report: found while getting the device gate ready (the maintainer's standing instruction to record bugs found in gate runs)
- Context: `testInlineEdits` and `testWindowsAndNames` timed out ("its undo name", "count set while hidden") whenever the app had run them before without being reinstalled, reading the status from the file or from the new status element alike; on a device, where the container stays, they would fail every time after the first.
- Cause: every document the iPad app opens autosaves (N4 slice 4), and a scripted launch's `--open-sample` opened the copy in Documents/Samples, so a test's edits were saved into the sample the next run opened, and into the person's samples.
- Fixed: 2026-11-21, the N4 device gate: with `--scratch-state`, `--open-sample` opens a fresh copy of the bundled sample in scratch (`Samples.scratchCopy`). Verified in the iPad simulator by running the five UI tests three times in one container (file, element, file). Still for the device: `device.sh ui`.

### B42. Exporting a value fails on a physical iPad: the export folder can't be written

- Reported: 2026-11-21
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: the conformance suite on the maintainer's iPad (debug build of the dev branch at `9b90500`), case "an array popped out by a person is a picture, and its numbers a grid paged by the core", step 13: `value.export {popout}` answers `<app container>/.cache/plumcraft/open/export/<pid>-<n>: Operation not permitted (os error 1)`. The core writes exports under `.cache` at the container's root, which an iOS app may not write; the simulator allows it, so the case passes there. Share, Save As and Drag Out use the same file, so they are expected to fail on the device too (not tried by hand).
- Issue: none
- Cause: the C ABI core never took the shell's `dirs.caches` (nor `dirs.scratch`): copies for apps and exported values went under the session's default, `HOME/.cache/plumcraft/open` on iOS (the desktop rule, `XDG_CACHE_HOME` or `~/.cache`, since iOS isn't `macos` to `cfg!`). On iOS `HOME` is the app's container, whose root a device app may not write; the simulator allows it, so every test there passed.
- Fixed: 2026-11-21, the N4 device gate (branch `native/n4-device-gate`): the core takes `dirs.caches` and `dirs.scratch` from the config (`plumcraft_ffi::core::Config::{caches, scratch}`): copies and exports go to `caches/open`, autosave's scratch files to `scratch/autosave`; the defaults that derive from `HOME` (open copies, the craft root, the HTTP host's CAS, preferences) use `Library/…` on iOS as on macOS. Found on the way: a short text (carried in its excerpt, with neither `value` nor `full`) exported and copied as nothing; fixed. Tests: `plumcraft-ffi::dirs` runs itself again with `HOME` read-only and every `dirs` root writable and takes values out every way the apps do (export, Share, the graph's Share, autosave in scratch and in the document's file, the open-value cache's root); it fails without the fix ("copies for apps go to …/home/Library/Caches/plumcraft/open"). In the iPad simulator with its data container's root made read-only (`chmod 555`, as a device has it): slice 6's build failed the same step as the iPad ("…/.cache/plumcraft/open/export…"), this branch's passed the whole suite, 52 of 52 (`claude-debug/native-n4/s7-b42-sim.sh`). Save As goes through the engine to the path the shell's ask gave (Documents) and the compiled-in engines save no formats, so it was not taken. Still for the device: the suite's export step on the iPad.

### B43. The graph's Parameters window is not reported open on a physical iPad

- Reported: 2026-11-21
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: the same run, case "the Cache Report and the graph's Parameters window are the core's: opened, read, edited and closed", step 11: `scene.get` has no `view.windows.params` where the case wants `true`. Passes on the Mac and in both simulators. One run only; whether it is timing on the slower debug build or a real difference is not looked into.
- Issue: none
- Cause: the iPad shell's, not the core's or the case's. Sheets the core opens and closes arrive faster than UIKit's transitions run (a script, the suite, a slow device). The Messages window (the case before this one in the suite) was dismissed while its presentation was still running, and the Cache Report was then presented over it; when the Messages sheet went it took the Cache Report and the Parameters sheet over that with it, and the shell, seeing them gone, told the core both were closed. Reproduced in the iPad simulator, launched as `device.sh` launches the app (Debug, `--scratch-state`): Messages opened and closed, then the Cache Report and Parameters opened at once, Parameters closed by itself within 0.11 s in 6 of 20 rounds (`claude-debug/native-n4/s7-b43-probe.py --messages`); the case alone looped 30 times never failed, which is why it passed everywhere but the device.
- Fixed: 2026-11-21, the N4 device gate: every presentation change the core drives on the iPad (core windows, Messages, Settings, a value's sheet replaced or closed) goes through one queue, `Presentations.whenSettled`, one at a time, in order, once nothing presented is mid-transition; a core window counts as closed by something else only after its own presentation has run. After it: 0 of 20 rounds of the probe, the case 30 of 30. New conformance case "windows opened as another closes stay open: the shell never closes what the core opened" with a new step form, `holds` (a condition that must stay so `for` a time, in both runners); it did not fail on slice 6's build in its one run there, so it catches the defect only some of the time. Still for the device: the suite on the iPad.

### B41. A run setting kept in the preferences is sometimes lost on the Linux test host

- Reported: 2026-11-20
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: `plumcraft-ffi::settings_autosave` `settings_are_the_cores_kept_in_its_prefs_and_every_document_follows_them`, `crates/ffi/tests/settings_autosave.rs:84`, added in N4 slice 4. After `theme.setToken`, `view.animationSpeed {ms: 1000}` and `lifecycle.appearance {dark: true}`, `settings.get {fromDisk: true}` gives Animation speed 500 (the default), not 1000. On the Linux test host it failed 1 of 10 alone at the dev branch before slice 5 and 6 of 10 at slice 5, which did not touch the settings code; it has not failed on the Mac. It may be the test's timing or a setting really lost when a document follows the shared look (`settings::follow` sets the animation speed from the preferences without a guard, unlike the run colours).
- Issue: none
- Cause: a defect in the editor, not in the test. `settings::after_command` wrote all three run settings (run colours, animation speed, Pick Engines Automatically) from whichever document ran a run command. Every document runs `engine.auto` when its first engine connects; a second document that connected after the first had set the speed, but before it had followed the shared look, wrote its own stale value back. A person with two windows could lose the setting the same way.
- Fixed: 2026-11-21, N4 slice 6: only the setting the command changed is kept (`Look::keep_run_pref`), and `settings.get {fromDisk}` reports the file's run settings. Test: `settings::tests::a_run_setting_changed_in_one_document_survives_another_that_has_not_followed_yet`. On the Linux test host the C ABI test file passed 100 of 100 runs at `e8a051b` (6 of 10 failed before).

### B36. A short clip's spectrogram is computed on the editor's thread

- Reported: 2026-11-20
- Report: "record bugs" (found while fixing B32)
- Context: `AudioView::sync` computes the spectrogram in place for clips under `SPEC_SYNC_SAMPLES`. A frame that shows a watched audio value took about 80 ms alone and 74 to 319 ms under load on the Linux test host (median 122 ms). It is not a wait for an engine, so B32's test no longer flags it.
- Issue: none
- Fixed: 2026-11-20, PR #131 (N4 slice 5): the spectrogram is computed on the media workers.

### B34. `a_save_in_the_app_feeds_the_nodes_downstream_and_runs` fails under the Linux test host's full parallel run

- Reported: 2026-11-20
- Report: "record the others"
- Context: `plumcraft-session` `doc_cmds::tests`: a poll returned `None` where the test expected the run's result. Failed in the baseline run of the dev branch at `b4a561b`; passes alone.
- Issue: none
- Cause (2026-11-20, confirmed): the test's saves came too close together for the file system's clock. Its last two saves (`{"doc": 1}` and then `{"doc": 4}`) are the same size as the save before each, and come 0.4 to 0.8 ms after it. The copy's clock on the Linux test host ticks once a millisecond. So often neither the size nor the modification time changed, and `poll_doc_edits` never saw the save. That is lines 784 and 791. An instrumented run printed an unchanged stamp before every failure. It is not load: on that host the test failed 91 of 100 runs alone, one at a time, and 123 of 200 with 40 copies at once. It passes on the Mac. That APFS's finer timestamps are the reason is inferred; it was not re-run there. Like B13, this is a test that met a clock. The session drops nothing in real use: a copy holds still for one 500 ms poll before it is read, so a later save lands at least 500 ms after the change the session last saw. Only a file system whose clock ticks more slowly than that (FAT, 2 s) could hide one. The test's polls run on a made-up clock, well under a millisecond apart.
- Fix: the test saves through a helper that writes the copy and, while its size and time are unchanged, waits a millisecond and writes it again, for at most 5 s. `doc_edits.rs` now states the limit. After: 0 of 100 alone and 0 of 200 with 40 copies at once.
- Fixed: 2026-11-20, PR #120

### B33. `hostile_files_never_panic_a_decoder` fails under the Linux test host's full parallel run

- Reported: 2026-11-20
- Report: "record the others"
- Context: `plumcraft-media::hostile`, `crates/media/tests/hostile.rs:98`: a decode went past the test's 5 s limit. Failed in the baseline run of the dev branch at `b4a561b`; passes alone.
- Issue: none
- Cause (2026-11-20, confirmed): the test, not a decoder. One input is slow. It is the empty SVG that claims `width='1e30'`, drawn by `svg::rasterise(&f, 1e9)`. The clamp holds: it is drawn at 4096 × 4096 (`MAX_RENDER_PIXELS`), not at its claim. Drawing those 16.8 million pixels and making them opaque takes 4.1 s in the test profile, against 0.23 s in release, so a 5 s limit on each file fails under load. Each file was timed alone through each decoder: every other input takes under 1 ms, apart from the first SVG parse, which loads the system fonts once (0.3 s). With 40 copies at once on the Linux test host it failed 37 of 200.
- Fix: the picture decoders stay timed against 5 s, which they reach only by believing a header. Each SVG raster is checked against the clamp that bounds its work (each side at most 4096, at most `MAX_RENDER_PIXELS`, a buffer of exactly w × h × 4) instead of being timed. After: 0 of 200 under the same load.
- Fixed: 2026-11-20, PR #119
### B32. `values_never_wait_for_an_engine` fails under the Linux test host's full parallel run

- Reported: 2026-11-20
- Report: "record the others"
- Context: `plumcraft-ui::engine_thread`, `crates/ui/tests/engine_thread.rs:546`: a spectrogram took longer than the test's 150 ms limit. Failed in the baseline run of the dev branch at `b4a561b` (80 cores, all tests in parallel); passes alone.
- Issue: none
- Cause (2026-11-20, confirmed): the test, not the editor. It held every frame under half of a 300 ms `resolve`, which measured the editor's own work in a frame, not a wait. With 40 copies at once on the Linux test host it failed 34 of 200: 32 in "watch n5" (the audio value; frames of 74 to 319 ms, median 122 ms), whose frame computes the Watched panel's spectrogram in place (`AudioView::sync`, clips under `SPEC_SYNC_SAMPLES`), and one each in the pop-out and compare phases. In all 200 runs no `resolve` was made on the editor's thread. Alone the audio frame is about 80 ms.
- Fix: the test holds each `resolve` at a gate until the editor has drawn three frames with it held, and fails if the gate has to give up on one (10 s): a frame that waited for an engine, directly or through a lock or channel, never returns while the call is held, however loaded the machine. Frame times are printed, not asserted. A deliberate fault (the Watched panel spinning until its value arrives) fails it. After: 0 of 200 under the same load, with audio frames up to 225 ms. The in-place spectrogram is still a stall of the editor's thread; moving it off the thread is a separate change.
- Fixed: 2026-11-20, PR #118
### B35. A blocking read's result could turn pending again (`blocking_reads_in_place_and_shares_the_cache` failed now and then)

- Reported: 2026-11-20
- Report: found by the B30 fix's full run on the Linux test host (failed at `crates/media/src/service.rs:1055`, "then cached"); 1 in 10 when run alone there.
- Cause (2026-11-20, confirmed): `MediaService::blocking` first calls `get`, which queues the request for a worker, then makes the value itself and keeps it as done, with the key still in the queue. A worker that reached the key after that set the done entry back to working and made it again, so a `get` meanwhile answered pending. Nothing else turns a done entry back. Not B31's cause (B31's test read too early); found while reading the same path. Reproduced 1 in 1,200 under load (0 in 300 alone); 0 in 1,200 after.
- Fix: a worker skips a queued request that is no longer queued (`run_one`). `a_queued_request_settled_in_place_is_not_made_again` fails without it (3 reads, want 2).
- Fixed: 2026-11-20, PR #117
- Issue: none

### B31. `a_request_answers_at_once_and_arrives_on_a_worker_with_a_wake` fails now and then on the Linux test host

- Reported: 2026-11-20
- Report: "record the others" (the four tests that fail only under the Linux host's full parallel run)
- Context: `plumcraft-media` `service::tests`. Failed once in the full run for N2 slice 2 (`85695df`) at `crates/media/src/service.rs:909`, "the shell was woken": the picture had arrived and the wake count was still zero. Passed 30 of 30 when run alone on the same host. The test is from N1 slice 6.
- Cause (2026-11-20, confirmed): the test read the wake counter too early. A worker keeps its result under the service's lock, lets the lock go, then calls the wake; the test asks without waiting for the wake, so a worker descheduled between the two lets it see the picture with the count still zero. That order (kept, then woken) is the right one: whoever a wake sends to ask again finds the result. Reproduced 112 times in 1,200 (60 copies at once, 20 rounds, with 40 busy loops alongside). Reading the path also found a real lost wake: when `MediaService::blocking` (`doc.open`, `value.open`) settled a request a worker was already making, the worker found it settled and neither kept its result nor woke, so a shell that had been told "pending" never heard; and a queued request settled that way was made a second time when a worker reached it.
- Fix: the test waits for the wake (10 s limit) and says what order is guaranteed; `blocking` wakes when it settles a request a worker had, after keeping it; a worker skips a queued request already settled. Three new tests: a wake always finds its result (200 rounds, the wake itself asks), and two for the in-place cases, which fail without the fix. After: 0 in 1,200 on the same load.
- Issue: none
- Fixed: 2026-11-20, PR #117
### B30. Video to frames fails with ffmpeg older than 5.1 (Ubuntu 22.04)

- Reported: 2026-11-20
- Report: "Bug:" (in answer to: the array engine's video reader passes `-fps_mode passthrough`, which ffmpeg only has from 5.1; Ubuntu 22.04 packages 4.4.2)
- Context: `video_goes_to_frames_and_back` (`crates/array/tests/channels.rs`) fails on the Linux test host with "video: /usr/bin/ffmpeg failed: Error splitting the argument list: Option not found". The option is passed at `crates/array/src/channels/video.rs:248`. GitHub's runner has a newer ffmpeg, so CI doesn't show it.
- Issue: #113
- Cause (2026-11-20, confirmed): `-fps_mode` was added in ffmpeg 5.1; before that the same setting is `-vsync passthrough`. It is the only option the video nodes pass that 4.4 lacks: on ffmpeg 4.4.2 the reader with `-vsync` (rgb24 and grayf32le), the ffprobe call, and the MP4 (libx264), MOV (prores_ks, profile 3, yuv422p10le) and GIF (palettegen/paletteuse, `-loop 0`, rgb and gray) writers all ran, and each written clip read back.
- Fix: the reader asks `ffmpeg -version` once per ffmpeg program and passes `-vsync` for a version before 5.1, else `-fps_mode`; an answer with no version in it (a git snapshot, or anything unparseable) gets `-fps_mode`. Test: `frames_pass_through_with_the_option_this_ffmpeg_has`.
- Fixed: 2026-11-20, PR #116

### B28. The hash gate fails on `main`'s Linux CI: "2 runtime commonkit v0.2 lines, want 1"

- Reported: 2026-11-19
- Report: "record the two main CI failures as bugs"
- Context: the `gate` job's "hash gates" step (`scripts/check-hash-gates.sh`) fails on `main` (latest `34f6aea`, run https://github.com/seantrue/plumcraft/actions/runs/43405581253) and on the native dev branch's pull requests, printing `hash gate: 2 runtime commonkit v0.2 lines, want 1; other versions: none`. The same script prints `hash gates: ok` on the Mac, on `main`'s code and on the dev branch's.
- Checked since (2026-11-19): on the Mac the script also passes under Rust 1.100.0, and `cargo tree … -i commonkit --target x86_64-unknown-linux-gnu` prints the same lines as for the Mac. So neither the Rust version alone nor the target's dependencies explains it. The runner's own `cargo tree` output has not been captured.
- Issue: #102
- Cause (2026-11-19, confirmed): CI sets `CARGO_TERM_COLOR=always`, so `cargo tree` wraps its `[build-dependencies]` heading in colour codes, and the script's `sed`, which cuts the output at that heading, no longer matches it: both `commonkit` lines are counted. Nothing to do with Linux or the Rust version; `CARGO_TERM_COLOR=always scripts/check-hash-gates.sh` fails the same way on a Mac. Found by making the script print the tree it counted and reading the runner's log.
- Fix: the script sets `CARGO_TERM_COLOR=never`, and prints the tree when the count is wrong.
- Fixed: 2026-11-19, on the native dev branch only, PR #109.

### B29. `mode2_python_kill_9_mid_run_recovers` fails on Linux CI: "the engine is not connected"

- Reported: 2026-11-19 (found by the controller on the native dev branch's pull request #101; not reported by the maintainer)
- Report: `plumcraft-text::conformance mode2_python_kill_9_mid_run_recovers` panicked at `crates/text/tests/conformance.rs:649` with `run: unavailable (-32006): the engine is not connected`, after 0.855 s, on run https://github.com/seantrue/plumcraft/actions/runs/44615077606 and again on its rerun: 2 of 2. Everything else passed (1,342 of 1,343). The Rust engine's twin, `mode2_kill_9_mid_run_recovers`, passed both times.
- Context: the same test passed on the four Linux runs before it that day (#98, #99, and #100 twice). The pull request's changes (N1 slices 4 and 5) touch nothing in `crates/text`, `crates/host`, `crates/conform`, `crates/proto` or `python/`. Not reproduced on the Mac: 40 runs alone and 48 runs twelve at a time all pass.
- Since (2026-11-19): intermittent, not constant. The Linux run of pull request #104 (the same code plus N1 slice 6) passed it, under md5 and b3: 2 failures in 7 Linux runs that day. #101 was merged into the dev branch with it failing (the maintainer's CPM, given to the slice 6 agent).
- Suspect, read from the code and not confirmed: in `crates/host/src/process.rs` the reader thread's end fails the run in flight before the supervisor has set the state to `EngineDown` (it reaps the process first, holding the `child` lock). `plumcraft_conform::kill_9_mid_run_recovers` gets the failed reply, then calls `wait_ready`, which returns at once on the stale `Connected`; `pid()` waits for the reap and is then not the first pid; the next `run` finds no writer.
- Open question: fix the host (state goes down before replies fail) or make the check wait for a new connection? The first changes what an editor sees after a crash, for the better if the suspicion is right.
- Issue: #103
- Cause (2026-11-19, confirmed; the suspect above had the order wrong): in `crates/host/src/process.rs` the supervisor failed the pending calls and live runs first and set the state to `EngineDown` after. Between the two, the check got its failed reply, `wait_ready` returned at once on the dead connection's `Connected`, `pid()` was `None` (already reaped, so "not the first pid" held), and the next `run` found no writer. A unit test that reads the state on hearing of the failure fails every time with that order.
- Fix: the state goes down first (`Inner::went_down`). An editor now sees the engine-state event before the failed run's reply, not after.
- Fixed: 2026-11-19, on the native dev branch only, PR pending. Not shown on Linux beyond CI passing: the failure was 2 runs in 9.

### B27. Eight editor kittests fail on `main`'s Linux CI, as in closed issue #89

- Reported: 2026-11-19
- Report: "record the two main CI failures as bugs"
- Context: found while comparing the native dev branch's pull requests (#98, #99) with `main`. The `gate` job's "cargo nextest, under md5 and then b3" step fails on every recent run of `main` (latest `34f6aea`, run https://github.com/seantrue/plumcraft/actions/runs/43405581253) with the same eight tests of `apps/plumcraft-editor/src/lib.rs`. `tests::audio_kittest`: `a_1khz_tone_peaks_in_its_spectrogram_bin`, `play_without_an_output_says_so_and_does_not_panic`, `the_audio_demo_runs_with_waveforms_and_an_audio_view`. `tests::engine_kittest`: `a_pick_adds_or_changes_an_engine_and_clear_starts_over`, `a_switch_cancels_the_run_in_flight`, `attach_connects_to_a_running_http_engine`, `switching_text_engines_runs_after_every_switch`, `text_to_audio_keeps_the_graph_and_reports_its_problems`. They pass on the Mac. Issue #89 ("CI gate: 8 editor kittests can't find the engine binaries (no engine binary at …/out/plumcraft-text-engine)") was closed as completed on 2026-11-12; the same eight still fail. The panic messages of the latest run have not been read against #89's, so that it is the same failure and not only the same tests is not confirmed. On the native dev branch the lookup (`engine_binary`) has moved to `crates/editor-core/src/engines.rs`.
- Found since (2026-11-19): it is #89's failure. `main`'s run panics with `no engine binary at /home/runner/work/plumcraft/plumcraft/target/debug/build/plumcraft-editor/62bf2a2969a8c725/out/plumcraft-text-engine` (and `…/plumcraft-audio-engine`). The test program on CI is at `target/debug/build/<package>/<hash>/out/`, not `target/debug/deps/`; `engine_binary` looks beside the program and, when the program is in `deps`, one folder up, so it finds nothing. CI installs the newest stable Rust, 1.100.0 (2026-11-09) on these runs; the Mac has cargo 1.99.0 and builds test programs in `target/debug/deps/`. That 1.100 moved them is inferred from those two facts, not read from its release notes.
- Also hit by: `plumcraft-ffi::switch starting_a_process_engine_never_holds_the_core_thread`, new in the native dev branch's pull request #100, which fails on Linux CI with the same message (`…/build/plumcraft-ffi/9d267b73553d9f8b/out/plumcraft-text-engine`).
- Issue: #89 (reopened 2026-11-19)
- Cause: confirmed with a scratch project built under both versions (`claude-debug/b27-layout/`): cargo 1.99 builds a test program in `<profile>/deps/`, cargo 1.100 in `<profile>/build/<package>/<hash>/out/`; the workspace's binaries are in `<profile>/` under both. Reproduced on the Mac with `cargo +1.100.0 test -p plumcraft-ffi --test switch`.
- Fix: `plumcraft_editor_core::engines::profile_dir` knows both layouts; `engine_binary` and the two test helpers that build a missing engine (`apps/plumcraft-editor/src/lib.rs`, `crates/ffi/tests/switch.rs`) use it. Test: `a_test_program_finds_engine_binaries_in_either_cargo_layout`. Under 1.100 on the Mac the switch test and the editor's kittests fail before and pass after; on Linux CI the pull request's run had no failing test.
- Fixed: 2026-11-19 on the dev branch `feat/native-n0` only, PR #100 (`b2bd46c`). `main` has its own copy of the lookup in `apps/plumcraft-editor/src/lib.rs` and still fails there; #89 stays open until the dev branch reaches `main`.

### B26. "Wrap seamlessly at the edges" on Simplex Noise's `tileable`

- Reported: 2026-11-11 (found while checking the kernel library's #65 against this port; filed as #84)
- Report: the `tileable` tooltip says "Wrap seamlessly at the edges", but simplex wraps its lattice in skewed coordinates, so the output repeats along the diagonal and its edges don't tile on x or y, on the kernel library's backends or here. Measured seams are the same size as with `tileable` off.
- Context: the kernel library made the same correction to its docs (its `simplex2d` docstring: "The output is not seamless along x or y"); real seamless tiling is the kernel library's #83 (a feature request), which would change this port too. impressionist's copy of the tooltip (its `docs/BUGS.md` B2) gets the same wording in its own branch.
- Issue: #84
- Cause: the tooltip described what tiling was meant to do, not what the kernel does.
- Fix: the tooltip (`crates/array/src/generate/mod.rs`) is now "Wrap the noise lattice; not seamless on x or y (smooth only)". Test: `simplex_tileable_tooltip_does_not_promise_seamless_edges` (`crates/array/tests/catalogue.rs`), which fails on the old text.
- Fixed: 2026-11-12, branch `fix/83-84-simplex-tile`

### B25. Simplex Noise swapped the tile periods on non-square tileable noise

- Reported: 2026-11-11 (found while checking the kernel library's #65 against this port; filed as #83)
- Report: `simplex2d` puts pixel (r, c) at (r / h, c / w) × frequency, but passed `tile_w = w / frequency` as the period of the row-based coordinate and `tile_h = h / frequency` as the column-based one's. The kernel library wraps the row-based coordinate with rows / frequency (its kernels' `width` is the rows). Square sizes, and `tileable` off, were unaffected; non-square tileable output differed from the kernel library by up to 0.72 (24 × 16, measured). Every `simplex.json` case was square, so none caught it.
- Context: the doc comment also said the numba kernel "is only right for" square sizes, which went stale when the kernel library's #75 (`29f8403`) fixed numba's loop.
- Issue: #83
- Cause: the port took the kernel library's `tile_w`/`tile_h` names as columns/rows, where the kernel library means rows/columns.
- Fix: `crates/array/src/generate/noise.rs`: the row-based coordinate wraps every `h / frequency` cells and the column-based one every `w / frequency` (`tile_rows`, `tile_cols`; `simplex2d_single`'s parameters renamed `tile_x`, `tile_y`). The doc comment now says what the kernels compute. `crates/array/tests/fixtures/generate/simplex.json` gains two non-square tileable cases (24 × 16 and 12 × 30) recorded from the kernel library's `main` (`6fa5e01`, numba backend) with the plum coordinator's probe `claude-debug/pc8384_simplex_fixture.py`, which first checked that the five existing cases still reproduce exactly; the existing cases are byte-identical. `simplex_matches_numba` reads `height` and `width` when a case has them and checks the shape. It fails on the old port (max error 0.72 at 24 × 16) and passes exactly (0) on the new.
- Fixed: 2026-11-12, branch `fix/83-84-simplex-tile`

### B24. xyY of black is NaN

- Reported: 2026-11-11 (found while porting colormath; filed upstream as the kernel library's #66)
- Report: the colour spaces gave x = y = NaN for black in xyY, and NaN RGB back, because colormath's OpenCL `xyz2xyY` divided 0 by 0, while its numba port gave 0.
- Context: the kernel library's #66 (now closed) settled it: an exact zero sum X+Y+Z has x = y = 0, and Y is kept. The maintainer chose that option "With check" (an explicit `sum != 0` test), and "Patch the 10 values directly" for this repository's fixture.
- Issue: the kernel library's #66 (closed)
- Cause: the ports copied the kernel's `x / sum`, `y / sum` with no zero check.
- Fix: `if sum != 0 { x/sum, y/sum } else { 0, 0 }`, Y kept, in the three ports: `crates/engine/src/color.rs` (`xyz_to_xyy`), `crates/array/src/colour/mod.rs` (`rgb_to`) and `python/src/plumcraft_engine/color.py` (`_xyz_to_xyy`, with `np.where`). Bit-identical to the kernel library's numbax `xyz2xyY` (jit and numpy fallback) on 100,000 random rows plus black, (0.5, -0.5, 0), -0 and (0.25, 0.25, -0.5). The inverses already gave XYZ 0 at y = 0 and are unchanged. `crates/engine/tests/fixtures/color.json`: the 10 nulls became 0.0, with nothing else changed (a JSON-aware comparison): `spaces.xyY.from_rgb` planes 0 and 1 (x, y) and `spaces.xyY.to_rgb` planes 0, 1 and 2 (r, g, b), each at samples 0 and 41. Tests: the fixture comparisons in `crates/engine/tests/color.rs` and `python/tests/test_color.py` (xyY) fail with the old ports and the patched fixture; the new `xyy_of_black_is_zero` (array) fails with the old port.
- Fixed: 2026-11-11, branch `fix/66-xyY-black`

### B23. A run marks every node queued, not only the ones it computes

- Reported: 2026-11-11 (found by the AI while evaluating run animation; the maintainer chose to fix it first)
- Report: "Go with proposed colors." (approving the run-animation proposal, which listed this as a fix to make first)
- Context: `RunState::request` (`crates/session/src/run.rs`) marked every node of the graph `Queued`, while the engines compute only what the request needs (`docs/scheduler.md` section 1). Clicking a node's title runs that node and what feeds it, yet every node, downstream ones included, showed "queued" until the reply, which then cleared their state, so a node outside the run lost its earlier "done" or "cached". The run's progress counted the whole graph too.
- Issue: none
- Cause: as above; the session never worked out which nodes a request needs.
- Fix: `run::needed` mirrors the scheduler's rule (the targets, or the sinks, plus watched nodes, and their ancestors; a wire into a bound input is dropped). Only those nodes are queued and counted, and the rest keep what they showed. Tests: `run::queued_tests` (four). Not checked in the real editor.
- Fixed: 2026-11-11, PR #81

### B22. Node thumbnails aren't in step with the latest run

- Reported: 2026-11-11 (from an application built on PlumCraft 0.6.1)
- Report: "node thumbnails are not synchronize"
- Context (the maintainer): a node's thumbnail lags behind that node's latest run, and thumbnails from different runs are shown side by side.
- Issue: #70
- Cause: three things in the session's run state (`crates/session/src/run.rs`). (1) A node's thumbnail was replaced only when a run sent a new one: a node done or cached in a run that made no thumbnail (an output with none, a picture that couldn't be read) kept an earlier run's picture under its new result. (2) While a run computed the graph again, each node kept its last thumbnail until it got there, by design so previews don't blink, but nothing marked it as the earlier run's: finished and waiting nodes looked alike. (3) An edit's stale mark went only at the run's reply, so a node this run had already finished stayed stale and its new thumbnail faded like the old ones until every node was done.
- Fix: the reply's thumbnail, or this run's preview, is the node's thumbnail, and none when the run made none. `NodeRun::thumb_generation` says which run a thumbnail came from, and `thumb_is_older` is true while a newer run is computing the node and hasn't sent one. The canvas fades such a thumbnail, excerpt or waveform, with a hover saying it is from the previous run (`canvas::thumb_faded`). A node stops being stale the moment the run made after the edit finishes it. Tests: `run::thumb_tests` (four, two fail without the fix) and `kittest_run_status.rs` (one). Checked in the real editor (`claude-debug/run_status_probe.py`, `claude-debug/b22/4-mid-run.png`): mid-run, the first node is up to date with the new text while the nodes waiting for the slow one keep their earlier excerpts, faded.
- Not fixed here: `stale_since` keeps the first edit that made a node stale. A node already stale when a run is requested and edited again during that run is taken as up to date when the run finishes it. This was already so at the reply, and now it happens when the node finishes.
- Fixed: 2026-11-11, PR #80

### B20. Can't tell which nodes are running

- Reported: 2026-11-11 (from an application built on PlumCraft 0.6.1)
- Report: "can't tell which nodes are actually running"
- Context: the node header shows a spinner while a node runs (`crates/ui/src/canvas.rs`, the run-status drawing).
- Issue: #68
- Cause: a node's stale mark outranked its run state. An edit makes the node and everything downstream stale, and the run that follows (every auto-run) computes them again, but the mark only went at the run's reply. Meanwhile each of them kept the stale border and the word "stale", queued and running alike; running showed only as the 12-point spinner in place of the state dot, and as a progress bar for a node that reports progress. Seen in the real editor (`claude-debug/b21/4-mid-run.png`): the running node looked like the queued ones.
- Fix: while a run computes a node (queued or running) its run state shows, not stale: the border in the queued or running colour (running a little wider), and the header says "queued", or "running" in the running colour with the percentage when the node reports one. `run_view::shows_stale` decides the stale label and border. A run that never reaches the node (cancelled, the engine gone) leaves it stale again. Tests: `kittest_run_status.rs` (two). Checked in the real editor (`claude-debug/run_status_probe.py`, `claude-debug/b20/4-mid-run.png`).
- Fixed: 2026-11-11, PR #79

### B21. A node that has never completed a run shows "stale"

- Reported: 2026-11-11 (from an application built on PlumCraft 0.6.1)
- Report: "stale is not a good status for node that has never completed a run"
- Context: the node header's run status (`crates/ui/src/canvas.rs`, the stale label and border).
- Issue: #69
- Design (the maintainer): a node with no completed run shows **not run**; stale stays for a node whose last result was invalidated by an edit.
- Cause: staleness was tracked for every node with a run record, whatever that run did. A node that failed, was skipped or cancelled, or that a run never reached, went "stale" on the next edit upstream, though it had no result to be out of date.
- Fix: `NodeRun::completed` remembers that a run has completed the node (done or cached), and `NodeRun::is_not_run` is true while none has and none is computing it, for a node never in a run, one left out of a run, or a failed, skipped or cancelled one after an edit. The header says "not run" for those, and the stale label and border are kept for a completed node. A failed, skipped or cancelled node still says so until an edit. `ui.inspect` gives `notRun` per node. Tests: `run::not_run_tests` (session) and `kittest_run_status.rs`. Checked in the real editor (`claude-debug/run_status_probe.py`, pictures in `claude-debug/b21/`): after an edit, the demo's failing node and the one after it say "not run", the rest "stale".
- Fixed: 2026-11-11, PR #78

### B10. Hashing makes content-addressed storage slow for large arrays

- Reported: 2026-11-05
- Report: "Fix the slow hashing issues for content addressed storage."
- Context: `ROADMAP.md` "Next: BLAKE3 handles for large local arrays (D3's measurement)". On an M4 Max with 0.2.1, a 12 MP float32 RGB array (144 MB) crossing between engines through the coordinator adds about 550–600 ms (36 MB: 130–190 ms), more than the nodes on either side; MD5 of 144 MB took 190 ms against 10 ms for BLAKE3 (`b3sum`, multithreaded). The rest of a crossing isn't profiled yet. Code: `Store::publish_array` and its read path (`crates/store`), `crossing_inputs` (`crates/coordinator/src/runner.rs`); the BLAKE3 key helper would go in commonkit (`../pycommonkit`), which D3 held back until a measurement favoured it.
- Issue: #28
- Design (the maintainer, 2026-11-10, of three offered): "Go to BLAKE3 now", then "Yes, publish commonkit".
- Cause: measured on this Mac with a release build (`claude-debug/b10-hash-timing`), 144 MB `<f4`: an array's publish hashed it twice (the engine's handle, then `Store::publish_array` again), about 350 ms of MD5, and a read checked it once more (170 ms): 528 ms, the 550–600 ms reported. BLAKE3 hashes it in 8–12 ms.
- Progress: commonkit 0.1.2 takes 64-digit digests (pycommonkit #17, #18). PlumCraft (`fix/28-blake3-handles`): `b3` handles in proto (shared vectors, checked against Python's `blake3`), the store publishes under the engine's handle without hashing again and checks a handle in its own algorithm, runs ask for `b3` (`RunRequest.algo`) when `hash.b3` was negotiated, and a coordinator clears it when an engine in the run lacks it. 144 MB publish and read back: 528 ms before, 29 ms with `b3` (355 ms with md5 hashed once). The Python SDK followed (`fix/28-blake3-python`): `blake3` as a dependency, `hash.b3` and `data.algos` in its hello, `b3` handles when a run asks, every handle checked in its own algorithm, pyuncommonkit 0.1.2.
- Fixed: 2026-11-10, PRs #74 (Rust) and #75 (Python)

### B18. Port and parameter labels have no translation table

- Reported: 2026-11-06
- Report: "port and parameter labels should have a \"translation\" table, allowing nicer display and formatting. (max_array_width -> Max. width) This table should be both claude AND maintainter editable."
- Context: the node canvas shows a port's name, or the engine's `label` for it, as it comes (`crates/ui/src/canvas.rs`, `port_label`). Raw parameter keys from engines and craft packs (`max_array_width`, `scaleStyles`) show as they are. The table should be a file both the maintainer and Claude can edit.
- Issue: #59
- Design (the maintainer, 2026-11-10, of three offered): "Per pack + user file". Each pack carries its own table, an optional user file overrides it, and core holds only the lookup, no names.
- Fixed: `plumcraft_proto::labels` reads a table (`[labels]`: a port name → label, or a node kind → a table of them; hostile entries skipped, in the hostile-input sweep). Layers, lowest first: the engine's own label; a craft pack's `[labels]` (applied by `plumcraft-craft` to its own nodes) or an application's table (`Session::use_labels`); the user's `labels.toml` beside the preferences. The session keeps the engines' catalogue and relabels it whenever a catalogue or a table changes; `labels.reload` reads the file again and `labels.show` lists what is in effect. PhotoCraft's pack labels `scaleStyles`. Checked in the real editor (`claude-debug/b18_labels_probe.py`): a `labels.toml` written while it ran relabelled the demo's ports on `labels.reload`.
- Not done here: a table for the research software's names (`max_array_width`) belongs to the research software's application, which calls `use_labels` with it.
- Fixed: 2026-11-10, PR #73

### B13. `pdfcraft_runs_a_chain_by_document_id` fails when a save straddles a second

- Reported: 2026-11-05
- Report: "yes, add as B13" (after the test stopped the first `release.sh` run for 0.2.4)
- Context: `crates/craft/tests/pack_pdfcraft.rs:94`, against the installed PdfCraft 0.17.0. The test runs the same chain twice and allows fewer than 16 differing bytes, as PdfCraft's saves depend on the clock in whole seconds through a few bytes of one compressed stream. In `release.sh`'s serial `cargo test` run it failed with "166 of 1498 bytes differ" (`claude-debug/release-0.2.4.log`); alone it passed 3 of 3, and the same commit passed nextest locally and on CI.
- Issue: none
- Seen again: 2026-11-06, in the full `cargo test` gate for #47 (status-bar messages); it passed alone.
- Cause: PdfCraft writes the save time, in whole seconds, as a `D:` date inside a Flate-compressed object stream. A save a second later re-compresses that stream, so hundreds of bytes move and the file can be a byte longer (reproduced with a 1.1 s pause: 487 bytes differ, 1496 against 1497 bytes). The test allowed fewer than 16 differing bytes at equal length.
- Fix: the test compares the two PDFs' streams, inflated, with dates masked and the cross-reference stream left out, and now always pauses past a second so the case it guards is the one it meets. It also checks that the comparison still tells two watermarks apart.
- Fixed: 2026-11-10, PR #71

### B19. A popout doesn't stay on top while the main window is being edited

- Reported: 2026-11-07
- Report: "popout shoud stay on top while main window is being edited"
- Context: reported from a session of the research software (the research software runs PlumCraft's editor); popouts are `crates/ui/src/popouts.rs`. Clicking into the main window to edit puts it in front of the popout.
- Issue: none
- Cause: pop-outs were ordinary windows (`WindowLevel::Normal`), so a click into the main window ordered it in front of them. Reproduced in the real editor (`claude-debug/b19_popout_on_top_probe.py`): after raising and clicking the main window, it was listed in front of the pop-out.
- Fix: while one of the editor's windows (the main one or a pop-out) has focus, every pop-out is `AlwaysOnTop`; when another application takes the focus they go back to `Normal`, so they don't float over other programs (`crates/ui/src/popouts.rs`). `ui.inspect` gives each pop-out's `on_top`. The probe: the pop-out stays in front (the floating level, layer 3) after the main window is clicked, drops to layer 0 with Finder in front, and floats again when the editor returns.
- Fixed: 2026-11-10, PR #72

### B17. Packs aren't versioned or chosen by the application's version

- Reported: 2026-11-06
- Report: "Packs should be versioned and selected against the engine (application) version."
- Context: the craft packs (`crates/craft/packs/*.toml`, `plumcraft_craft::manifest`). Just before this, the newly installed craft apps added command parameters (PhotoCraft 0.20.2, SoundCraft 0.17.1, VectorCraft), and the packs were edited to match on branch `craft-baselines`. A pack has no version of its own and doesn't say which application versions it fits, so one pack serves every installed version.
- Issue: #56
- Fixed: 2026-11-06, PR #58

### B16. Not everything tabular can be converted to a TABLE

- Reported: 2026-11-06
- Report: "anything tabular should be convertible to a TABLE (csv, grid objects, etc)"
- Context: Convert (`core.convert`, `docs/research/20-convert.md`) and the TABLE core type. Today the text engine converts Text ↔ Table and Array ↔ Table (6.3), and has CSV readers. Whether a CSV file value, a GridCraft workbook or range, or other tabular craft and pack types have a converter to TABLE is not checked yet.
- Cause: CSV files, CSV text, arrays and GridCraft workbooks (Export CSV → CSV to Table) already reached TABLE. JSON had no converter, so rows such as GridCraft's Read range went JSON → Text → Text to Table and were parsed as CSV. CSV readers knew only commas.
- Issue: #54
- Fixed: 2026-11-06, PR #55

### B15. Not every value can be turned into an array; a binary file can't be cut and reshaped

- Reported: 2026-11-06
- Report: "All values should be reducible to arrays, even if just a bytes array. value -> bytes array -> slice -> shaped array of dtype should be allow truncation of a binary file and casting it to a user determined shape."
- Context: core types and Convert (`core.convert`, `docs/research/20-convert.md`), and the array engine (`crates/array`). The chain asked for is any value → its bytes as a 1-D `uint8` ARRAY → a slice (to cut a binary file short) → a reinterpretation as an array of a chosen dtype and shape.
- Issue: #50
- Fixed: 2026-11-06, PR #53

### B14. A port's value isn't shown on hover unless a wire is connected; clicking a port doesn't watch it

- Reported: 2026-11-06
- Report: "ports should hover reveal the available value even if no wire is connected, click should watch the port value"
- Context: the node canvas (`crates/ui/src/canvas.rs`), seen while working in a private batch app's window on PlumCraft 0.4.0. Today a node's output is watched by clicking the node's title, which watches its default output (`run.watch {node, port?}`).
- Issue: #49
- Fixed: 2026-11-06, PR #52

### B2. No installer for Intel (x86_64) Macs

- Reported: 2026-11-05
- Report: "No installer for x86 arch on Mac"
- Context: releases carry only `PlumCraft-VERSION-macos-arm64.dmg`, as `packaging/release.sh` builds this machine's architecture. On 2026-11-05 `packaging/macos.sh` gained `CARGO_BUILD_TARGET=x86_64-apple-darwin` (uncommitted) and a test `PlumCraft-0.2.2-macos-x86_64.dmg` was built from the working tree, but it was never run on an Intel Mac and `release.sh` doesn't build or upload it.
- Fixed: 2026-11-05, PR #41. `release.sh` on a Mac builds both DMGs (`arm64` in `target/release`, `x86_64` with `CARGO_BUILD_TARGET`), checks both Rust targets are installed before building, and checksums and uploads both. `macos.sh` checks every binary's architecture with `lipo` and runs the Intel CLI when Rosetta 2 is installed. Both 0.2.3 DMGs built, signed and verified on the M4 Max. **Not yet run on an Intel Mac** (this Mac has no Rosetta 2).
- Issue: #23

### B12. No viewer for STL files

- Reported: 2026-11-05
- Report: "Add viewer for STL files"
- Context: the viewers (`crates/ui/src/viewers/`, registry in `mod.rs`) have nothing for 3D meshes, so an STL value gets the hex dump. The protocol already anticipates meshes: `Media::Mesh` (`crates/proto/src/catalogue.rs`), a `MeshLod` rendition (`crates/proto/src/display.rs:140`), and `model/stl` ↔ `stl` in `crates/proto/src/media.rs:32`. Not yet known: where the STL came from (a file, an engine such as SolidCraft's export) and whether it was ASCII or binary STL.
- Fixed: 2026-11-05, PR #40. A `mesh` viewer (`crates/ui/src/viewers/mesh.rs`) for values typed `model/stl`, binary or ASCII: flat-shaded on the CPU with a z-buffer, turned by a drag, zoomed by scrolling, fitted by a double-click or Fit. The bounded reader is `plumcraft_proto::mesh` and joins the hostile sweep. Both are adapted from SolidCraft (MIT, `THIRD-PARTY.md`) after research found nothing ready to use with egui 0.36 and wgpu. `MeshLod` renditions aren't used yet.
- Issue: #38

### B3. With automatic engines on, the Engine menu still offers every implementation

- Reported: 2026-11-05
- Report: "When \"preferences->engines->select engine automatically\" is enabled, the \"engines->select engine\" should just select the engine, not offer the list of implementations"
- Context: the setting is Settings › Engines › Pick Engines Automatically (`crates/ui/src/settings_view.rs:225`, `engine.auto`); the Engine menu's list of engines and their hostings comes from `engine_choices_menu` (`crates/ui/src/engine_view.rs:423`, called from `crates/ui/src/lib.rs:1509`). Expected: with the setting on, choosing an engine starts it the fastest way it can be hosted (in-process, then a process, then HTTP) without a submenu of implementations.
- Fixed: 2026-11-05, PR #39. With the setting on, `engine_choices_menu` shows each engine as one item running `engine.use` with no mode, which the session already resolves to the fastest hosting (`Engines::best`); with it off, the submenu stays. Tests: `crates/ui/tests/kittest_engine_menu.rs` (the automatic case fails without the fix).
- Issue: #24

### B11. `a_panicking_node_rebuilds_the_engine` raced the engine's rebuild on CI

- Reported: 2026-11-05
- Report: "Add as B11" (after the test failed the first CI run of both #30 and #31)
- Context: `crates/text/tests/conformance.rs`, `a_panicking_node_rebuilds_the_engine`: a node panics and the host rebuilds the engine. It failed the first runs of PR #30 and #31 (runs 42102535342, 42102561899) and both runs of #36 under cargo-nextest (42122511058, 42123346098), and passed on rerun and locally.
- First diagnosis, wrong: too short a 5 s wait.
- Cause: after the panicking run's reply, the test asserted `events.iter().any(EngineDown) || builds == 2`. The `EngineDown` half could never hold: the host pushes it as a session event (no run id), and `collect` polls the run's events only. The other half raced the host, which replies and then rebuilds (`crates/engine/src/host.rs`: `going_down`, `finish`, `rebuild`). Reproduced every time by making the rebuild take 300 ms before it counts; under load on the M4 Max it never failed in 40 runs.
- Fixed: 2026-11-05, branch `fix/33-panic-rebuild-race`. The test reads the session's events and asserts `EngineDown` comes before the run's reply, the order the host guarantees, then waits for the rebuild as before. The slow rebuild stays in the test, so the reply always comes first; 0 of 30 runs failed under load.
- Issue: #33

### B6. npexact has no contraction rule for numpy's runtime-dispatched SIMD loops

- Reported: 2026-11-05
- Report: "In PlumCraft, but remember as bug for future fix in npexact" (on fixing B5)
- Repo: npexact, in the Rust kernel library's repository; not PlumCraft's to fix
- Context: `npexact::random::Contraction::host_default()` (npexact 0.1.1, `src/random/mod.rs:223`) decides fused-or-plain by architecture at compile time: fused on aarch64, plain elsewhere. That fits numpy.random's C, which the wheels compile once, but not numpy's SIMD ufunc loops, which numpy picks at run time and which fuse on x86-64 with FMA3 and AVX2 (B5). PlumCraft borrowed the random rule for Mandelbrot's complex multiply and got x86-64 wrong. B5 is fixed in PlumCraft with its own `numpy_complex_fma()` (`crates/array/src/generate/noise.rs`); npexact should offer that rule (a runtime "numpy's SIMD contraction" beside `Contraction`, with fixtures from both hosts), and PlumCraft should then call it instead.
- Issue: the Rust kernel library's #10
- Fixed in npexact: 2026-11-05, the Rust kernel library's #11 (`5a51589`), with the recorder in the kernel library's #74. `npexact::complex::Mul` has `array_default()`, decided at run time (fused on arm64, and on x86-64 with FMA3 and AVX2), and `scalar_default()`, decided at build time: numpy scalars are plain on x86-64. npexact 0.1.2 published it (tag `npexact-v0.1.2`).
- Fixed in PlumCraft: 2026-11-05, branch `fix/b6-npexact-complex`. The Mandelbrot kernel calls `npexact::complex::Mul::array_default()` and `mul_c64` (npexact 0.1.2) in place of its own `numpy_complex_fma()`; the golden test passes on the M4 Max and on the Linux test host.

### B4. No built-in viewer for XML files

- Reported: 2026-11-05
- Report: "No build in viewer for XML files."
- Context: the built-in viewers are in `crates/ui/src/viewers/` (`basic`, `grid`, `pdf`, `plot`, `wrap`, `yaml`); there is a YAML one but nothing for XML. SVG, which is XML, is drawn as a picture (`crates/ui/src/media.rs`). Read here as "built-in"; no detail yet on where the XML came from (a file node, an engine's value) or what it showed instead.
- Issue: #26
- Fixed: 2026-11-05, branch `fix/26-xml-viewer`. A new `xml` viewer (`crates/ui/src/viewers/xml.rs`) takes XML text (a declaration, or an element after comments) and bytes typed `application/xml`, `text/xml` or `…+xml` (SVG keeps its own), ahead of Text. It shows an outline of elements, attributes, text and comments, with the source a checkbox away and a parse error's position. roxmltree parses it (already in the build through usvg), with DTDs refused, a node limit, and the outline bounded in depth, nodes and bytes; tests sweep truncations, flipped bytes and an entity bomb. Checked in the real editor with `claude-debug/xml_viewer_probe.py` (XML into the demo's `story` node, popped out with `viewer: "xml"`).

### B1. A popout covers the whole screen when the editor is full screen

- Reported: 2026-11-05
- Report: "popout when editor is full screen covers up whole screen"
- Context: the editor (`target/debug/plumcraft-editor`, working tree with the theme and Settings changes) on macOS in full screen; popouts are `crates/ui/src/popouts.rs`
- Issue: #22
- Cause: not a full-screen window of its own. With the editor full screen, macOS's automatic window tabbing made the new pop-out a tab of the editor's window, so it filled the screen and hid the editor. Reproduced in the real editor with `claude-debug/popout_fullscreen_probe.py` (full screen through the accessibility attribute, then `view.popout`): the tab bar showed "Demo — PlumCraft" and "shout.text — PlumCraft".
- Fixed: 2026-11-05, branch `fix/22-popout-fullscreen`. `no_window_tabbing()` (`crates/ui/src/lib.rs`, from `setup_context`) turns automatic tabbing off with objc2-app-kit's safe `NSWindow::setAllowsAutomaticWindowTabbing`; the probe then shows the pop-out as a 760×592 window of its own over the full-screen editor.

### B5. The Mandelbrot kernel doesn't fuse on x86-64, though numpy does

- Reported: 2026-11-05
- Report: "fix test data" (on the first Linux CI run failing `mandelbrot_matches_numba`)
- Context: CI run https://github.com/seantrue/plumcraft/actions/runs/42081191424 (PR #25, ubuntu-latest, x86_64): `crates/array/tests/generate.rs:85`, "mandelbrot 33×21: 9 of 693 pixels differ, max error 9.5e-1"; the 40×30 case matched. Cargo stopped at this test binary, so the rest of the Rust tests and the Python tests didn't run.
- First diagnosis, wrong: that the golden was arm64-only and x86-64 needed its own. On the maintainer's "Record mandelbrot test data on [the Linux test host]", the golden recorded on the Linux test host (x86-64, same numpy 2.5.3 and numba 0.68.0, identical numba kernel) matched the committed arm64 one in every pixel of all three cases (`claude-debug/mandelbrot_golden_host.py`).
- Cause: numpy's complex64 multiply fuses its multiply-adds on any CPU numpy dispatches its AVX2/FMA3 loop to, not just on arm64. `claude-debug/complex64_fma_probe.py` matched a fused model on 100% of 200,000 products on both machines and an unfused one on 71%. The kernel (`crates/array/src/generate/noise.rs:163`) picks fusion with `npexact::random::Contraction::host_default()`, a compile-time rule (fused on aarch64, plain elsewhere) that fits numpy.random's compiled C, not the runtime-dispatched SIMD loop. So on x86-64 it runs plain while numpy fuses. The test data is right; the kernel is wrong on x86-64.
- Fix: fuse when numpy's loop would: aarch64, or x86-64 with FMA3 and AVX2 at run time. The rule may belong in npexact, beside `Contraction`. The Linux test host and the CI runners both have FMA3 and AVX2.
- Issue: #27
- Fixed: 2026-11-05, PR #25 (`ba68b25`); #27 closed

### B7. The editor's audio tests time out on Linux CI

- Reported: 2026-11-05
- Report: found by the first Linux CI run with `--no-fail-fast` (run https://github.com/seantrue/plumcraft/actions/runs/42083352370); the maintainer asked to record and fix it ("Go for 2,3-5")
- Context: `apps/plumcraft-editor/src/lib.rs` `tests::audio_kittest`: `play_without_an_output_says_so_and_does_not_panic`, `a_1khz_tone_peaks_in_its_spectrogram_bin` and `the_audio_demo_runs_with_waveforms_and_an_audio_view` each panic at `lib.rs:2084` ("timed out waiting for the watched audio", "… the tone's spectrogram", "… the watched audio and its spectrogram") after 30 s. ALSA on the runner finds no sound card. The first test is meant to pass with no output device.
- Cause, found on the Linux test host: not the missing sound card (`ALSA_CARD=NoSuchCard` passes, as does 4 CPUs) but the runner's disk. commonkit's store declines to publish when the disk has under `MIN_FREE_GB` (8 GB) free, and after the workspace's build the runner has less, so the watched audio never arrives. With a fresh store root and `COMMONKIT_STORE_MIN_FREE_GB=100000`, the Linux test host times out the same three tests the same way. Same cause as B9.
- Fix: CI sets `COMMONKIT_STORE_MIN_FREE_GB=2` for the job. The store behaves as designed; that the editor waits rather than saying the value is unavailable may be worth a look.
- Issue: none
- Fixed: 2026-11-05, PR #25 (`ba68b25`), the CI floor

### B8. An SVG label in `sans-serif` draws nothing on Linux

- Reported: 2026-11-05
- Report: as B7 ("Go for 2,3-5")
- Context: `crates/svg/src/lib.rs:169` `text_is_drawn_with_the_systems_fonts`: "0 inked pixels" on ubuntu-latest, although `fonts()` found fonts (the test returns early when there are none). The label is `<text font-family="sans-serif">Plum</text>`; on the Mac it draws.
- Cause, likely: fontdb takes the generic families from fontconfig on Linux (on the Linux test host `sans-serif` is FreeSans, installed, so it passes there), and nothing checks that the family it names is installed. Where it isn't, the label matches no face and isn't drawn. Not reproduced off the runner; the next CI run's failure message names the family if this is wrong.
- Fix: `installed_generics` in `crates/svg/src/lib.rs` points each generic family that names a missing family at an installed one, with a test that sets them all to a missing family.
- Issue: none
- Fixed: 2026-11-05, PR #25 (`ba68b25`); the next Linux CI run passed it

### B9. Large JSON crosses the Python binding as `opaque`, not `blob`, on Linux

- Reported: 2026-11-05
- Report: as B7 ("Go for 2,3-5")
- Context: `python/tests/test_audio.py:69` `test_large_json_crosses_as_a_blob`: `assert ('opaque' == 'blob')` at line 77 on ubuntu-latest; 217 of 218 Python tests pass there, and all pass on the Mac.
- Cause: the same as B7. The store declines the large JSON because the runner's disk is under commonkit's 8 GB floor, so `Plane` reports it `opaque` (`python/src/plumcraft_engine/data.py:239`). Reproduced on the Linux test host with a fresh store root and `COMMONKIT_STORE_MIN_FREE_GB=100000`; without the raised floor it passes.
- Fix: as B7.
- Issue: none
- Fixed: 2026-11-05, PR #25 (`ba68b25`), the CI floor
