# 26. Data views: arrays and tables as pictures, plots and charts in the native shells

**Research, 2026-11-24, for B154. Slice 1 is built (view modes, histograms). Sections 11 and 12, the capability review and the revised plan, were added overnight on 2026-11-24.**

The maintainer, 2026-11-24 (B154):

- "regression in views of arrays (the egui version supported plots, view as image, heatmap, etc). These are important. For tabular data, visualizing selected columns is also important."
- "B154: Start this as a parallel research task. Survey first. Look at seaborn. Histograms. Line charts, stacked line charts, column charts, stacked column charts,  treemaps. Interactive is good (selecting columns, rows, etc; for interrogating data, zooming into ranges)"

## 1. Summary

- **The regression is real and larger than the plot.** Of the 17 ways the egui editor shows an array or a table (section 2), the native shells have 4 in full and 2 in part. **Eleven are missing on every native shell**: the plot of a table, the plot of a 1-D series, both of their hover previews, the array's view modes (auto, colour, stretch, clip, heat, labels, angle), the channel histograms, colour by value in the numbers grid and in the table, the grid's number format, Compare's exact difference, and the mesh. The several-picture view has lost its shared zoom and probe, and the statistics their NaN count. Mac, iPad, iPhone and Windows are alike in every row, because they draw what the core's `popout.get` gives and the core gives none of these.
- **Three of them are floor rows, and one is graded wrongly.** F51 "View modes" (`docs/plan.md`: the engine's `views`, the research software's `auto color stretch clip heat labels angle`) is marked met on the Mac and iPad in note 24 on the strength of the viewer kinds (View As), not the view modes; no native shell can choose one. The research software's viewer also had 256-bin histograms with a log toggle and tables plotted as points or segments (note 04, rows "View modes", "Channel names, exact min/max, 256-bin histograms, log toggle", "Tables plotted as points or segments").
- **The data is mostly there already.** The engines send per-channel histograms in every array's statistics, and a value's offered views are in the session; the core drops both on the way to a native shell. `run.view` is already a command. So the first two slices are small: carry what the core has, and draw it.
- **Charts for tables are new work, for every shell including egui.** egui plots only an (N, 2) or position table, with no choice of columns. The maintainer's list (histogram, line, stacked line, column, stacked column, treemap) and "selecting columns, rows, … zooming into ranges" are new everywhere.
- **Recommended architecture: the core lays charts out, every shell paints them** (option B, section 6). A chart is a small declarative spec (a mark and encodings, as Vega-Lite and seaborn's objects interface have it) kept by the core; the core aggregates the data on the media service's workers (binning, stacking, downsampling, treemap layout), lays out marks, axes and labels into a display list, and answers hit tests, as the canvas scene already works. Each shell paints the list with its own 2D drawing (SwiftUI `Canvas` or Core Graphics, Win2D, egui's painter) and its own text and accessibility. Every choice and gesture is a core command. Swift Charts is good but would be one of three chart implementations, has no treemap and no built-in zoom, and Windows has no stable chart control.
- **Order:** the regressions first (a table's plot, a 1-D plot, view modes and histograms), then the six required chart kinds, then interaction.

## 2. Survey: what egui shows today, and what each native shell shows

Read on the dev branch at `921fd65`, 2026-11-24. "egui" is the egui editor's Watched panel and its pop-outs; "core offers" is what the editor core gives a native shell: the viewer kinds of `plumcraft_editor_core::viewers::offered` (`crates/editor-core/src/viewers.rs:48`), the extras of `popout_extra` (`:112`) and the pages of `viewer.data` (`:197`). The Mac, the iPad and the iPhone share `PlumCraftShell/Viewers.swift` for every viewer but the single picture, which each app draws in its own zoomable view (`PlumCraftMac/App/Popouts.swift:267`, `PlumCraftPad/App/Popouts.swift:405`); the iPhone is the iPad app in compact width, so it has one column. Windows is `native/windows/src/PlumCraft.App/Viewers.cs`, routed in `Popouts.cs:209`.

How each cell was established: **T** a named test exercises it (named in the evidence); **R** read from the code, no named test; a dash, not there. All file paths are under `crates/` unless they start with `native/`.

| # | View | egui | Core offers | Mac | iPad, iPhone | Windows | Evidence |
|---|---|---|---|---|---|---|---|
| 1 | An array with one picture: Fit, 1:1, pinch or scroll zoom, the probe (the value's own numbers under the pointer), a pinned probe, a channel (RGBA, RGB, R, G, B, A), more detail on zoom | Yes (T) | Yes: `viewer.zoom`, `viewer.probe`, `viewer.picture`, `viewer.data probe` | Yes (T) | Yes (T) | Yes (T) | egui `ui/src/picture_tools.rs:221` (Fit, 1:1, −, +), `kittest_viewer_tools.rs:269`, `:296`, `:743`; conformance "an array popped out by a person is a picture, and its numbers a grid paged by the core" and "a picture popped out by a person shows a channel, reads its own numbers and asks for detail"; Mac `Popouts.swift:379-460`, iPad `Popouts.swift:525-595`, Windows `Viewers.cs:297-520`. The shells have no − and + buttons (pinch instead) |
| 2 | An array with several pictures (one per channel, captioned), sharing one zoom, pan and probe, two to a row | Yes (R) | The pictures (`views`), no more | Pictures in a grid, fitted; no zoom, no probe (R) | Same (R) | Same (R) | egui `ui/src/array_view.rs:98-119`; Apple `Viewers.swift:841-871` (a `LazyVGrid` of `LentPicture`), routed there only when there is more than one view (`Popouts.swift:227`, iPad `:364`); Windows `Viewers.cs:523-562` |
| 3 | **View modes** of an array's pictures: `auto`, `color`, `stretch`, `clip`, `heat` (Turbo), `labels`, `angle` with its `turn` (floor F51) | Yes: a View bar, `run.view` (R; `run.view` itself T) | The command `run.view` exists (`session/src/run_cmds.rs:146`); the views offered are **not** in `popout.get` | – | – | – | egui `ui/src/array_view.rs:65-94`, tips `:138`; offered list `session/src/run_cmds.rs:134`; the engine draws them, `engine/src/arrays.rs:9-16`, Turbo `:232`; `kittest_viewer_tools.rs:376` sends `run.view {view: "clip"}`; no native file sends `run.view` (searched) |
| 4 | Channel statistics: each channel's exact min, max and NaN count | Yes (R) | min, max, NaN, as text (`viewers.rs:129-138`) | min and max only, NaN dropped (R) | Same (R) | Same (R) | egui `array_view.rs:171-173`; Apple `Viewers.swift:848`, Windows `Viewers.cs:90` |
| 5 | **Channel histograms**: 256 bins a channel (a table's: a column), over its own range, log scale on a checkbox | Yes (R) | Dropped: the engine sends them in `Display.stats`, `popout_extra` keeps only name, min, max, NaN | – | – | – | egui `array_view.rs:122-130`, drawn `:171-196`; engine `engine/src/arrays.rs:286-313` (`BINS` 256, per column for a table); core `editor-core/src/viewers.rs:129-138` |
| 6 | The numbers grid (`array.grid`): paged rows, a window of columns, the plane of a 3-D or deeper array | Yes, scrolling, a picker per extra axis (R) | Yes: 500 rows a page, 64 columns a window, one flattened plane index | Yes (T) | Yes (T) | Yes (T) | egui `ui/src/viewers/grid.rs:172-190`; core `viewers.rs:371-411`; conformance "a wide array's numbers are shown 64 columns at a time, in the core's window"; Apple `Viewers.swift:558-636`, Windows `Viewers.cs:571-815` |
| 7 | The grid's number format (`%.4g`, `%.3f`, `%.6f`, `%.3e`, `%d`, or typed) | Yes (R) | No: the core's number text only (`viewers.rs:82`) | – | – | – | egui `viewers/grid.rs:19`, `:176`; `docs/shell-interface.md` 13.2 proposes `viewer.grid {format}` (open question 5) |
| 8 | **Colour by value** in the grid (a heat from the panel's colour to the chart colour, over the slice's range) | Yes (R) | No | – | – | – | egui `viewers/grid.rs:181`, `:275-279`, `viewers/basic.rs:175` (`heat`, its test `:594`); proposed `viewer.grid {colored}` (13.2) |
| 9 | A table (`table.grid`): a sortable, paged grid | Yes, sorted in the UI (T) | Yes: `viewer.table`, sorted and paged by the core on workers, a table-flagged array too since B152 | Yes (T) | Yes (T) | Yes (T) | `kittest_viewers.rs:225`; conformance "a table popped out by a person is sorted and paged by the core"; Windows `PanelsTests.cs:87`; B152 on `fix/b152-table-array` |
| 10 | **Colour by value** in a table | Yes (R) | No | – | – | – | egui `viewers/basic.rs:160-215` |
| 11 | **Plot of a table** (`array.plot`): an (N, k) table array plotted as points (columns `x`, `y`, or the two of an (N, 2)) or segments (`x0 y0 x1 y1`), drawn to one scale y down when the spans agree; drag to pan, zoom, hover coordinates (egui_plot's defaults); at most 50,000 rows | Yes (T) | **Not offered** (left out of `offered`; the core's test of the two registries excludes it by name) | – | – | – | egui `ui/src/viewers/mod.rs:157`, `viewers/plot.rs:58-109`, `:173-184`, `kittest_viewer_tools.rs:590`, `:606`; core `viewers.rs:48-73`, exclusion `ui/src/viewers/mod.rs:471`. B152's histogram (a 256 × 2 table with `centre`, `count`) is a plot first in egui, numbers in the shells |
| 12 | Its preview: the plot on a wire's or a port's hover | Yes (R) | No picture: the engine makes no thumbnail of a table (`engine/src/arrays.rs:38-41`, `:281`) | Summary only (R) | Summary only (R) | Summary only (R) | egui `viewers/plot.rs:141-149` |
| 13 | **Plot of a 1-D series** (a bar a value when they fit, else each pixel column spans the min and max of its values) | Yes, in Pictures and the Watched panel (R) | Offered `array.pictures` and `array.grid`; no plot data | "This array has no picture: see its values." (R) | Same (R) | An empty picture list (R) | egui `ui/src/viewers/wrap.rs:115-127`, `ui/src/watched.rs:265-273`, `:437-472` (its hostile test `:521`); Apple `Viewers.swift:858` |
| 14 | Its preview on hover | Yes (R) | No (no thumbnail of a 1-D array, `arrays.rs:368`) | – | – | – | egui `viewers/wrap.rs:104-113` |
| 15 | Compare: A and B side by side, or split by a divider | Yes (T) | Yes: `viewer.pin`, `viewer.compare`, `compare.get` | Yes (T) | Yes (T) | Yes (T) | `kittest_viewer_tools.rs:702`; conformance "Compare pins two values popped out by a person, side by side or split, and unpins them" |
| 16 | **Compare's exact difference** \|A−B\| (the channels' mean, or a picture a channel), with its probe | Yes (T) | The state has it (`session/src/viewer_cmds.rs:36-50`); the shells say "The exact difference of A and B is the egui editor's for now" | – | – | – | `kittest_viewer_tools.rs:469`, `:505`, `ui/src/compare_view.rs:555`; Apple `PlumCraftShell/Reports.swift:425`, Windows `CoreWindows.cs:398`; floor F53 "partly" in note 24 |
| 17 | A mesh (an STL): shaded, turned by a drag, zoomed | Yes (T) | **Not offered** (excluded with `array.plot`) | Engine's picture and summary (R) | Same (R) | Same (R) | egui `ui/src/viewers/mod.rs:154`, `viewers/mesh.rs`, `kittest_viewers.rs:261`; the shells' fallback, contract 13.3 |

Not counted above: the Watched panel's own small table (`ui/src/watched.rs:402-430`) draws a bar beside each row for the last numeric column, a one-column bar chart. By reading, the panel shows it only when no viewer tab takes over, and the `table.grid` tab does for every table, so it is rarely seen; no test drives it in the panel. The docked Watched panel itself is not a regression: the native shells replaced it with pop-outs by decision (the maintainer, N3 exit card 14, note 24).

**The count.** 17 ways; the native shells have 4 in full (rows 1, 6, 9, 15) and 2 in part (2, 4); **11 are missing on every native shell** (3, 5, 7, 8, 10, 11, 12, 13, 14, 16, 17), the same on Mac, iPad, iPhone and Windows. Of the 11, the core already holds the data or the command for three (3's `run.view`, 5's histograms, 16's difference state) and drops or hides it.

**Why the shells are alike.** Each native shell draws only what `popout.get` and `viewer.data` give, and the core gives a fixed list (`NATIVE`, `viewers.rs:77`); a viewer outside it falls back to the engine's picture (contract 13.3). So every row above is a core gap first and a shell gap second.

**The floor.** F51 "View modes" (`docs/plan.md`: "Spec 7.2 `views`", "Listed by the engine, not Rust") is the array view modes of row 3, which the research software's viewer had (`docs/research/04-visualisation.md`, "View modes `auto color stretch clip heat labels angle` (+ `turn`)"). Note 24's floor table marks it met on the Mac and the iPad on the evidence of View As (the viewer kinds), which is a different thing. It should read not met. The same note-04 inventory lists "Channel names, exact min/max, 256-bin histograms, log toggle" (row 5) and "Tables plotted as points or segments" (row 11) as what the research software's viewer did, so those rows fall under the floor too (F50, F54 as `docs/plan.md` words them).

## 3. What the engines produce

The chart set should fit the values that actually arrive. From this repository's engines (and the DuckDB engine, `../plumcraft-duckdb`):

| Shape | What makes it | Names | Best seen as |
|---|---|---|---|
| A picture, (H, W, C) float, a colour space or channel names | Most `array.*` nodes (Load Image, Gaussian Blur, Convert Color Space, Stack …) | Channel names (`L`, `a`, `b`) | Picture, view modes, histograms |
| A single plane, (H, W) | Gradient, Sobel Magnitude, FFT magnitude, Mandelbrot, Simplex Noise, Pick Channel | One | Heatmap (`heat` view), numbers grid with colour |
| A label map, (H, W) integer, `labels` | Connected Components, Segment Edges' regions (`array/src/generate/regions.rs:100`) | `label` | `labels` view; a count per label as columns |
| A histogram, (bins, 2) table, `<f4` | Histogram (`array/src/math/mod.rs:400-416`, 256 bins by default, up to 65,536) | `centre`, `count` | Column chart (B152's value) |
| Points, (N, 2 + k) table | Random Points, Mask Points (`array/src/generate/points.rs:36`, `:53`) | `x`, `y`, `c3` … | Scatter, same scale, y down |
| Segments, (N, 4) table | Zero Crossings (`array/src/filters/mod.rs:427-430`), Grid Segments, Trace Contours | `x0 y0 x1 y1` | Segments over the picture's frame |
| A product, (M, N) table, unnamed | Matrix Multiply of a table (`array/src/math/mod.rs:540`) | `c1` … | Heatmap of the matrix, or lines |
| A table from a table, (rows, k) `<f8` | Table to Array (`text/src/pairs.rs:225`) | The table's numeric columns | Any chart below |
| Samples, (frames, channels) float, not a table | Audio to Array (`audio/src/io.rs:222`) | Channel names | Line chart (it is drawn today as a tall thin picture) |
| A 1-D series | Slice of a row, an engine's vector | None | Line or column chart |
| TABLE, columnar JSON, typed columns | Text to Table, CSV and JSON readers (`text.*`), Transients (`time` FLOAT, `frame` INT) and Regions (`start`, `end`) in `audio/src/analysis.rs:202`, `:227`; DuckDB's Read File and SQL Query | `INT`, `FLOAT`, `BOOL`, `STRING`, and from DuckDB also `DATE`, `TIME`, `TIMESTAMP` (as ISO text) and `JSON` | Every chart in section 4; categories from STRING, time from the three time types |
| Per-channel statistics on every numeric array | The SDKs (`engine/src/arrays.rs:286`) | The channels' or columns' names | Histograms beside the picture |

Sizes. A TABLE is inline up to 64 KiB (`limits.max_inline`) and otherwise a blob; the core's table reader takes up to 50,000,000 rows, 4,096 columns and 512 MiB (`media/src/table.rs:15-19`), and egui's plot reads at most 50,000 rows (`ui/src/viewers/plot.rs:21`). A graph has at most about 500 nodes, but one table may have tens of millions of rows, so nothing a chart draws may scale with the rows: it scales with the pixels.

What this means for the chart set: histograms and columns are what the array engine already makes; points and segments are its geometry; a time axis matters for DuckDB's tables and the audio engine's tables; categories (STRING columns) are what grouping, stacking and treemaps need, and today only DuckDB and the CSV readers make them.

## 4. Chart vocabulary

### 4.1 What seaborn teaches

seaborn 0.13.2 (January 2024, still the current release) sorts its plots by the question they answer: **relational** (`scatterplot`, `lineplot`), **distributional** (`histplot`, `kdeplot`, `ecdfplot`, `rugplot`), **categorical** (`barplot`, `countplot`, `boxplot`, `violinplot`, `boxenplot`, `stripplot`, `swarmplot`, `pointplot`), **regression** and **matrix** (`heatmap`, `clustermap`). Five ideas are worth taking:

1. **One kind of question, one family; the kind of mark is a choice within it.** The figure-level functions (`relplot`, `displot`, `catplot`) take `kind=` and choose the mark. PlumCraft's equivalent is one Chart viewer with a mark picker, not one viewer per chart type.
2. **Columns map to roles, not to series by hand.** `x`, `y`, then the semantic roles `hue` (colour), `size` (width or area) and `style` (dashes or markers), then `col` and `row` (small multiples). A person picks a column for each role. This is the whole of "selecting columns".
3. **The statistics are part of the chart.** `lineplot` takes the mean of the y values at each x and draws an interval (`errorbar` of `"ci"`, `"pi"`, `"se"` or `"sd"`); `histplot` has `stat` (count, frequency, probability, percent, density) and `multiple` (`layer`, `dodge`, `stack`, `fill`). So aggregation is a property of the spec, done where the data is: in PlumCraft's core.
4. **The grammar under it.** The `seaborn.objects` interface (0.12, still called "experimental and incomplete") is mark plus stat plus move: `so.Plot(df, x=…, y=…, color=…).add(so.Bar(), so.Agg(), so.Dodge())`, with marks `Bar`, `Line`, `Area`, `Dot`, `Range`, stats `Agg`, `Est`, `Count`, `Hist`, `KDE`, and moves `Stack`, `Dodge`, `Norm`, `Jitter`. That is exactly the shape a saved spec should have.
5. **Sensible defaults from the data's types.** A numeric x with a numeric y is a scatter or a line; a category x is bars; a single numeric column is a histogram.

What not to take: matplotlib's figure model and styling surface; bootstrap confidence intervals by default (1,000 resamples over millions of rows is a cost a viewer should not pay unasked: offer `sd` and `se`, and `ci` on request); regression fits and `clustermap`'s clustering (analysis that belongs in an engine node, not in a viewer); faceting in the first slices (small multiples are valuable but a later step).

### 4.2 The same thing as a declarative spec

Vega-Lite (6.5.0, October 2026) and Observable Plot (0.6.19, November 2026) both express a chart as **a mark plus encodings**, each encoding a field with a type and an optional aggregate, bin or time unit, and stacking as a property:

```json
{"mark": "bar",
 "encoding": {"x": {"field": "age", "type": "nominal"},
              "y": {"aggregate": "sum", "field": "pop", "type": "quantitative", "stack": "zero"},
              "color": {"field": "sex", "type": "nominal"}},
 "params": [{"name": "grid", "select": "interval", "bind": "scales"}]}
```

In Observable Plot the same is `Plot.barY(data, {x: "age", y: "pop", fill: "sex"})`, stacked implicitly. Vega-Lite's selections are part of the spec too: a `point` or `interval` selection, and `bind: "scales"` for zoom and pan. Neither has a treemap: Vega-Lite's issue has been open since 2017 and a proposed mark (#9784) is unmerged; Vega has a `treemap` transform (squarify by default). Observable Plot's `tree` mark is a node-link tree, not a treemap.

**What PlumCraft should save** is a small subset of this, in its own words and validated by the core:

```json
{"mark": "bar", "x": {"column": "region"}, "y": {"column": "amount", "agg": "sum"},
 "color": {"column": "product"}, "stack": "zero", "range": {"x": null, "y": null}}
```

Field types come from the TABLE's column types (INT and FLOAT quantitative; STRING and BOOL nominal; DATE, TIME and TIMESTAMP temporal), so a person rarely sets one; for an array, the columns are the array's `names` (or `c1` …). The spec is data the core checks, never code a shell runs.

### 4.3 The kinds, what each needs, and how columns map

Required by the maintainer: histogram, line, stacked line, column, stacked column, treemap. The rest ranked.

| Kind | Data it needs | Columns onto roles | In the core | Rank |
|---|---|---|---|---|
| Histogram | One numeric column (or an array's channel) | x = the column; color = an optional category (layered, stacked or side by side, seaborn's `multiple`) | Bins over the range (count, or density); 256 by default like the engine's; an already-binned table (the Histogram node's `centre`, `count`) drawn as columns as it is | Required |
| Line | x numeric or temporal, one or more y numeric | x; y = one column, or several (each a series); color = a category that splits one y into series | Sorted by x; repeated x take the mean (seaborn's rule), optional `sd` or `se` band; downsampled to min and max per pixel column (M4) past the width | Required |
| Stacked line (stacked area) | As line, with several series | As line; `stack` zero or normalize | Series aligned on x (missing as zero), stacked in legend order | Required |
| Column (bar) | x category (or binned number), y numeric (or none: a count) | x; y with an aggregate (sum, mean, count, min, max) | Group by x, aggregate | Required |
| Stacked column | As column, plus a category | x; y; color = the stacked category; `stack` zero or normalize | Group by x and color, aggregate, stack | Required |
| Treemap | One to three category columns (a hierarchy), a size column (or none: a count) | path = the categories, outermost first; size; color = the top level, or a numeric column | Group, sum, squarified layout (Bruls, Huizing and van Wijk 2000), labels where they fit; a click goes one level down | Required |
| Grouped column | As stacked column | color side by side (seaborn's `dodge`) | As stacked, offset instead of stacked | 1: one option on the column kinds |
| Scatter | Two numeric columns | x, y; color; size | Past a few thousand points a density picture (an image mark) instead of points | 2: today's point and segment plot is a scatter |
| Heatmap of a 2-D array | An (H, W) array | rows, columns, value | Already the engine's `heat` view and the colour grid (rows 3 and 8); as a chart only to add axes and a colour scale | 3 |
| Heatmap of a pivoted table | Two category (or binned) columns and a value | x, y, value with an aggregate | Pivot, aggregate | 4 |
| ECDF | One numeric column | x; color | Sorted values; exact, no parameters | 5 |
| Box | A numeric column by a category | x category, y numeric | Quartiles per group | 6 |
| KDE | One numeric column | x; color | A bandwidth to choose; a sample past a size | 7 |
| Violin | As box | As box | KDE per group | 8 |

## 5. Interaction, and what each platform gives

### 5.1 What a person does, as commands

"Interactive is good (selecting columns, rows, etc; for interrogating data, zooming into ranges)". Each of these is a core command with a pop-out id, its state in `popout.get`, so a script drives and reads it:

| Action | Command (proposed) | Notes |
|---|---|---|
| Choose the kind of chart and the columns for x, y, color, size, path; aggregate, stack, bins | `viewer.chart {popout, mark?, x?, y?, color?, size?, path?, agg?, stack?, bins?}` | Refused with a reason when a column doesn't exist or its type doesn't fit; defaults from the types (4.1 point 5) |
| Zoom and pan into a range; reset | `viewer.range {popout, x?: [lo, hi] \| null, y?}` | A pinch, a scroll or a drag on an axis is the shell's while it moves and one command when it ends, as a picture's zoom is; the data is reduced again at the new range |
| Read exact values under the pointer | `viewer.data {popout, part: "hit", x, y}` | The core's hit test answers with the mark, its rows (or group) and their exact values in the core's number text; hovering itself is not a command, as the probe's hover is not today |
| Select rows: a click on a mark, a brush over a range, or rows picked in the table | `viewer.select {popout, rows? \| brush? \| clear?}` | The selection belongs to the value (node and port), so its table and its chart show the same rows (linked views) |
| Sort and filter the table | `viewer.table {sort, desc}` (exists); `viewer.table {filter}` and "only the selected rows" | The chart draws what the filter keeps |
| A chart's small form on a face or a hover | none | The core paints a thumbnail from the same display list |

### 5.2 Swift Charts (macOS, iOS, iPadOS)

Checked against Apple's documentation, 2026-11-24. The Apple shells' floor is iOS 18 and macOS 15 (`project.yml`, `Package.swift`), so everything below except 3D is available.

- **Marks:** `BarMark`, `LineMark`, `AreaMark`, `PointMark`, `RuleMark`, `RectangleMark` (iOS 16, macOS 13), `SectorMark` (iOS 17, macOS 14). Stacking by `MarkStackingMethod` (`.standard`, `.normalized`, `.center`, `.unstacked`) and grouping by `.position(by:)`, both iOS 16.
- **Large data:** the vectorized plots `LinePlot`, `AreaPlot`, `PointPlot`, `BarPlot`, `RectanglePlot`, `RulePlot`, `SectorPlot` and function plots (iOS 18, macOS 15). Apple's session on them (WWDC24 10155) gives no point counts, only "larger datasets"; no documented limit was found. The core would still have to reduce ten million rows to thousands.
- **Selection:** `chartXSelection(value:)`, `chartXSelection(range:)`, `chartYSelection`, `chartAngleSelection` (iOS 17, macOS 14). They report a value or a range of the axis, not rows; rows are the core's to find.
- **Scrolling:** `chartScrollableAxes`, `chartXVisibleDomain(length:)`, `chartScrollPosition`, `chartScrollTargetBehavior` (iOS 17, macOS 14).
- **Zoom: not built in.** Pinch zoom must be built from `MagnifyGesture` with the visible domain and scroll position; a developer forum thread of September 2026 asks Apple for it (no reply), and another reports jumps while keeping the pinch anchored.
- **Treemap: none.** No treemap mark or plot in the framework.
- **Accessibility:** marks get accessibility labels and Audio Graphs automatically. A chart drawn some other way gets the same through `accessibilityChartDescriptor(_:)` and `AXChartDescriptor` (iOS 15, macOS 12), which is what option B would use.
- **2025:** the only Swift Charts addition listed is 3D (`Chart3D`, `SurfacePlot`, OS 26).

### 5.3 Windows (WinUI 3)

- **Built in:** the Windows App SDK 2.5.4-experimental (September 2026) added a Chart control (line, area and bar series; category, date-time and numeric axes; legends). Microsoft says experimental APIs may change or be removed and are not for production; zoom, pan, tooltips and selection are not mentioned; no treemap. The Windows shell is on the stable 2.6.5 (`PlumCraft.App.csproj`), so this is not available to it today. The Windows Community Toolkit has no chart.
- **LiveCharts2** (`LiveChartsCore.SkiaSharpView.WinUI`): MIT, 2.2.0 (19 November 2026), first stable 2.0.0 in April 2026, very active; zoom, pan and tooltips; renders through SkiaSharp, whose package ships Windows ARM64 natives. Its large-data virtualisation is in a separate "backers" assembly not on NuGet. Open issues on clicks starting a pan and tooltips sticking. No treemap found.
- **ScottPlot** (`ScottPlot.WinUI`): MIT, 5.1.59 (June 2026); signal plots for millions of points, a heatmap with colour bars, histogram, box; its own pan and zoom. ARM64 inferred from SkiaSharp, not checked. No treemap in its cookbook.
- **OxyPlot:** no official WinUI package; the community port's repository is archived.
- **Commercial:** Syncfusion's WinUI charts (no WinUI treemap; a community licence for small companies), Telerik UI for WinUI (pan and zoom; no WinUI treemap); DevExpress's WinUI suite is discontinued.
- **Win2D** (`Microsoft.Graphics.Win2D` 1.4.0, March 2026, ARM64 natives in the package) is already the Windows shell's drawing (`Viewers.cs` uses `CanvasDevice`), so painting a display list there adds no dependency.

No WinUI treemap was found from any vendor. On Windows a treemap is our layout whatever is chosen.

### 5.4 The core drawing charts

- **plotters** (MIT, 0.3.7, September 2024, no release since): bitmap and SVG back ends, and a `DrawingBackend` trait whose vector calls (`draw_line`, `draw_rect`, `draw_path`, `draw_circle`, `fill_polygon`, `draw_text`, `estimate_text_size`) could be recorded as a display list. Its axes and ticks are its own, and its interaction is none.
- **egui_plot** (0.38.0, October 2026, "looking for maintainer") paints with egui and produces no neutral scene.
- **charts-rs** renders to SVG and PNG only.
- **No Rust crate was found that emits a platform-neutral chart scene.** The parts a core chart engine needs are each small and well described: nice ticks, linear, log and time scales, stacking, binning, squarified treemap layout (the `treemap` crate is unmaintained since 2019, so the layout would be written or ported from d3-hierarchy), and downsampling: M4 (min, max, first and last per pixel column, exact when rasterised; Jugel et al. 2014), which the media service's waveform already does in part, and LTTB or MinMaxLTTB (`minmaxlttb`, MIT, maintained) for smoother lines.

## 6. Architecture: three ways to put a chart on the screen

What every option shares, because the rules and the sizes force it:

- **The chart's state is the core's.** Which columns are x, y, series, size and group, the kind of mark, the zoom range, the selected rows, the sort and the filter are commands (golden rule 4; "a shell-side user action is a defect"), kept beside today's `viewer.table` and `viewer.grid` state (`session/src/viewer_state.rs`) and read back in `popout.get`'s `state`. A script sets and reads them as a person does, and every shell shows the same chart for the same state. Continuous motion inside a gesture (the brush while it is dragged, a pinch in progress) is the shell's until it lifts, as a picture's pinch is today (`docs/shell-interface.md` 13.2.1, "where the line falls").
- **The data is reduced by the core, off the main thread, paged.** A chart never sees ten million rows. The media service already derives a sorted permutation (`Want::Sorted`), a waveform of min and max per column (`Want::Waveform`, which is M4's idea) and a spectrogram picture, each keyed, cached and pending until ready (`media/src/service.rs:69-130`). Charts add derived kinds of the same sort: bins of a column, a group-by with an aggregate, a stack, a pivot, a line downsampled to the pixels (min and max per pixel column, or LTTB), a treemap's rectangles. The reply is pending until read, exactly as `viewer.data` is.
- **Number and date text is the core's** (`viewers::number`), as it is in every viewer: a shell never formats a tick label.

### Option A: each shell draws with its platform's chart toolkit

The core serves the spec and the reduced data as pages; Swift Charts draws on the Mac, iPad and iPhone; Windows draws with a library (LiveCharts2 or ScottPlot) or its own Win2D code; egui keeps egui_plot.

- **For:** the most native look on Apple, with Swift Charts' own accessibility (each mark labelled, Audio Graphs) and its selection and scrolling modifiers.
- **Against:** three chart implementations (Swift Charts, a Windows library, egui_plot) that must agree on axes, ticks, stacking order, colours and what a hit means; conformance can then check only the data, not the chart. Swift Charts has no treemap, so its layout is ours anyway, and its selection modifiers report a value, not rows. Windows has no stable chart control (the Windows App SDK's Chart is experimental, not for production, and without a treemap), so there it is either a third-party dependency or Option B's drawing written in C# alone. Each new chart kind is three pieces of work.

### Option B: the core lays the chart out into a display list every shell paints (recommended)

The core turns spec and reduced data into a platform-neutral scene of the chart: the plot rectangle, marks (rects, polylines, areas, points, segments, text), axes with their ticks and tick text, gridlines, a legend, and an accessibility outline (the chart's title, each axis's label and range, each series' name and a few summary numbers, a label per mark or per group). Coordinates are in the chart's own points; the shell scales them to its view. Hit tests (hover read-out, a tap on a mark, a brush) go back to the core, which answers with rows, as the canvas does today.

- **For:** one implementation of everything that is hard to get the same twice (scales, nice ticks, stacking, binning, treemap layout, colour assignment, hit testing); the conformance suite compares display lists from a fixture, numbers within 1e-3 as it already does, so every shell is held to the same chart; egui paints the same list with its painter, which keeps egui as the first shell and its kittests as the regression net; a new chart kind is core work plus nothing in the shells unless it brings a new primitive. It is the canvas's architecture (note 24: "the platform recognises, the core decides"), so the patterns are known: scene snapshots, a wake, borrowed bytes, hit tests on geometry.
- **Against:** the look is ours, not Swift Charts'; it can be made to match the platform's (its fonts, label colours, accent and grid colours from the platform's tokens, which the shells already map), but the work of matching is ours. Accessibility must be built from the outline on each platform: on Apple an `AXChartDescriptor` (which Audio Graphs read) and accessibility elements per mark, on Windows UI Automation peers as the canvas's `CanvasPeer.cs` already has. Text measurement: the core needs the size of tick and legend text to leave room; it can estimate from the platform's reported font metrics (a few numbers a shell sends once) and let the shell place text by anchor and alignment.

### Option C: the core renders the chart as a picture

As it already does for spectrograms and meshes (`derive:spectrogram`, `derive:mesh`), the core rasterises the chart (with plotters or its own drawing) and the shells show a picture.

- **For:** cheapest; one look everywhere; nothing new in a shell; heatmaps of arrays are pictures already (the engine's `heat` view).
- **Against:** text that isn't the platform's and blurs when scaled; every hover, zoom or theme change is a new render; accessibility only as a description; the least native look, against the maintainer's preference for the platform's look over egui's.

### Weighed

| Concern | A: toolkits | B: core display list | C: core pictures |
|---|---|---|---|
| Golden rule 4, every action a command | Possible, but each toolkit's gestures must be translated back to commands three times | Natural: the core owns hits and state | Natural |
| Native look | Best on Apple; Windows depends on a library | Good: platform text, colours, controls around it | Poor |
| Windows (no chart control) | A third-party library or our own code | Win2D, already used by the Windows shell | Nothing to do |
| Treemap | Ours on every platform anyway | Once, in the core | Once, in the core |
| Tens of millions of rows | Core reduces; toolkit draws thousands | Core reduces and lays out | Core reduces and draws |
| Conformance shared by all shells | Data only | Data and the chart | Data and a picture hash |
| Accessibility | Built in on Apple; per library on Windows | Built from the core's outline on each platform | A text description |
| egui | Keeps egui_plot | Paints the same list | Shows the picture |
| Cost to add a chart kind | Three implementations | One | One |

**Recommendation: B, with C kept for what is a picture by nature.** The array view modes and an array's heatmap stay pictures made by the engine (as today); a scatter of more points than pixels becomes a density picture made by the core inside the display list (an image primitive). Everything with axes, text and marks a person points at is a display list.

**What it costs.** A chart crate in the core with no toolkit in it (scales, ticks, stacking, bins, layout of marks, axes and legend, the treemap, hit tests): of the order of the canvas scene's first slices, a few thousand lines with its tests. A painter per shell for perhaps eight primitives (rect, polyline, area, point, segment, text, image, clip) and the accessibility mapping: a few hundred lines each in Swift, C# and egui. New derived kinds on the media service, each a pending-until-ready job like the sorted permutation. The chart spec in the contract (`docs/shell-interface.md`, `docs/control-protocol.md`) and its schema.

## 7. A staged plan

Each slice ends with a gate a test checks and a launch as a person (memory: "a feature works only after launching it as a person"). Slices 1 and 2 bring back the regressions with almost no new design; slice 3 starts the chart engine on the regressed plot, so the architecture is proven on a known case before the new kinds.

| Slice | What | Gate (named test) | Launch as a person |
|---|---|---|---|
| 1. View modes and histograms | `popout.get` carries the views offered and the one shown (`view: {id, params}`, `views: [id]`), and each channel's histogram (counts, lo, hi) and NaN count. Each shell adds a View picker beside View As that sends `run.view` (with `turn` for `angle`), draws the histograms as small columns under the stats (log scale a command, `viewer.array {log}`, as 13.2 proposes), and shows NaN counts. F51 re-graded | Conformance "an array's view is chosen through the core and its pictures change" (heat, then labels; the pictures' handles differ); a core unit test of the extras; the egui kittest unchanged | Mac, iPad, Windows: Gradient popped out, View › Heat, the picture turns Turbo; Histogram node's input shows its histogram |
| 2. Several pictures, the colour grid, the difference | The several-picture view gets the single picture's zoom, pan and probe, shared (the core's `viewer.zoom` and `viewer.probe` already take a pop-out); `viewer.grid {colored, format}` and `viewer.table {colored}` (open question 5) and their painting; Compare's difference as a `derive:compare` picture the shells show | Conformance cases for each command's state and page (the grid page carries each cell's 0-to-1 position in its range); Compare's case adds `how: "difference"` | Lab picture's three channels zoomed together; a grid in colour; Compare's third tab |
| 3. The plot, again, on the new engine | The chart crate's first marks (point, segment, rule, rect, polyline), linear scales, nice ticks, the core's number text; `array.plot` offered and in `NATIVE`; the display list in `viewer.data {part: "chart"}`; painters in egui, Swift and C#; downsampling past 50,000 rows by the core | Display-list fixtures (points, segments, same-scale y-down, a 256 × 2 histogram) compared in Rust, the Swift runner and the C# runner; the registry test stops excluding `array.plot` | B152's histogram popped out on the Mac and Windows shows as a plot; Random Points over a picture's frame |
| 4. 1-D series and hover previews | A 1-D array (and audio's (frames, channels)) as a line, min and max per pixel column past the width; the chart's small form as a face or hover picture (the core paints a thumbnail from the same list) | Fixtures; the scene's hover preview names a chart thumbnail | Slice of a row popped out as a line; hover over its wire |
| 5. Columns and histograms of a table, with column choice | The spec as a command, `viewer.chart {popout, mark, x, y, color, …}`, validated against the table's columns and types; histogram (bins in the core, `bins` and `range` in the spec), column, grouped and stacked column (sum by default; count when no y) | Conformance "a table's chart is chosen by command and drawn from the core's aggregate" on a 10,000,000-row fixture made on the fly (the page arrives pending, then ready, in under a second on the Mac); fixtures per kind | A DuckDB table popped out on the Mac, Chart, x = category, y = amount, colour = region, Stack |
| 6. Line and stacked area | Line (several y columns, or one y split by a colour column), stacked area (`stack: zero`, `normalize` optional), time axes from DATE, TIME and TIMESTAMP | Fixtures, a time-axis tick test (days, months, years) | An audio Transients table as a line over time; a stacked area of three columns |
| 7. Treemap | Group by one to three category columns, a size column (count when none), squarified layout in the core, labels where they fit, a click goes one level down (a command) | Fixtures of the layout (the rectangles' areas within 1e-6 of their shares) | A DuckDB table by region and product |
| 8. Zoom, hover, reset | `viewer.range {popout, x?: [lo, hi], y?}` and Reset; a drag on an axis or a pinch ends in one `viewer.range`; hover read-out by the core's hit test (`viewer.data {part: "hit", x, y}`: the mark, its row or group and its exact values) | Conformance: a range set by command is the range drawn and the data re-reduced at the new resolution | Zoom into a million-point line until single samples show |
| 9. Selection and linked views | `viewer.select {popout, rows | brush | clear}`; the selection belongs to the value (node and port), so a table pop-out and a chart pop-out of one port show the same rows; the table can show only the selected rows; the chart highlights them | Conformance: brush on the chart, the table's page lists exactly those rows; select rows in the table, the chart highlights them | Brush a range on a histogram; the table shows its rows |
| 10. Optional kinds, by the ranking in section 4 | Scatter with density, heatmap of a pivot, ECDF, box, KDE, violin | Fixtures each | Each launched once |

The chart spec saved with the graph (decision 3) belongs in slice 5, when there is first a choice worth saving.

## 8. Decisions for the maintainer

Recommendation first in each.

| # | Question | Options |
|---|---|---|
| 1 | How charts are drawn | **A: the core lays charts out, each shell paints them (option B)**; B: each shell's chart toolkit (Swift Charts; a library on Windows; egui_plot); C: the core makes pictures |
| 2 | Order | **A: the regressions first (slices 1 to 4), then the required kinds, then interaction**; B: the required chart kinds first |
| 3 | Where a chart's choices are kept | **A: saved with the graph, per output port, in the graph's `meta` (not in the result hash), and undoable**; B: the session only, lost on close, as a table's sort is today |
| 4 | Whether egui changes | **A: egui paints the same display list (its own plot retired), so its tests guard the charts**; B: egui keeps egui_plot and its own plot |
| 5 | The Windows drawing | **A: Win2D, which the Windows shell already uses**; B: a library (ScottPlot, MIT) for the plot area only |
| 6 | The optional kinds | **A: the ranking in section 4 (grouped columns and scatter with the required kinds; heatmap of a pivot, ECDF, box next; KDE and violin last)**; B: required kinds only for now |
| 7 | A selection's reach | **A: a selection belongs to the value (node and port), shared by its table and chart pop-outs**; B: each pop-out its own |

**Decided (the maintainer, 2026-11-24):** given the seven decisions above with the recommendation for each, they answered "Yes to all". So option A in every row: the core lays charts out and each shell paints them; the regressions first; a chart's choices saved with the graph and undoable; egui paints the same display list; Win2D on Windows; the ranking of section 4 for the optional kinds; a selection belongs to the value.

Decision 3 rests on a fact worth stating: `graph_info.hash`, the hash of what makes the result, leaves out the graph's `meta` (decision 1B of the save work, `docs/interface.md` 15), so a chart spec kept there changes no saved result's hash.

## 9. What could not be verified

- No Apple source gives a point count at which Swift Charts slows; WWDC24 10155 says only "larger datasets".
- Whether the experimental WinUI Chart has zoom, pan, tooltips or selection, and its class names.
- LiveCharts2: its zoom and pan documentation page (404), the backers package's terms today, and whether it has a treemap.
- ScottPlot on Windows ARM64 (inferred from SkiaSharp's natives, not run); its lack of a treemap only from the cookbook's index.
- DevExpress's WinUI discontinuation from search summaries, not its own pages; Telerik and Syncfusion WinUI pricing.
- Steinarsson's thesis page returned 403 (the PDF link is as the tsdownsample README cites it); the ACM page for M4 returned 403 (a mirror was read).
- Observable Plot's site returned 429; its documentation was read from its source on GitHub.
- In this repository: egui_plot's interaction in the Plot viewer (pan, zoom, hover coordinates) is its defaults as read, not driven by a named test; nothing here was launched, so every native "R" cell in section 2 is from reading, and the counts in section 2 should be confirmed by launching each shell once on the slice-1 branch.

## 10. Sources

- seaborn: https://seaborn.pydata.org/api.html ; https://seaborn.pydata.org/tutorial/function_overview.html ; https://seaborn.pydata.org/tutorial/objects_interface.html ; https://seaborn.pydata.org/tutorial/error_bars.html ; https://seaborn.pydata.org/generated/seaborn.histplot.html ; https://seaborn.pydata.org/generated/seaborn.lineplot.html ; https://seaborn.pydata.org/whatsnew/index.html ; https://pypi.org/pypi/seaborn/json
- Swift Charts: https://developer.apple.com/documentation/charts ; https://developer.apple.com/documentation/charts/lineplot ; https://developer.apple.com/documentation/charts/chart3d ; https://developer.apple.com/documentation/charts/sectormark ; https://developer.apple.com/documentation/charts/markstackingmethod ; https://developer.apple.com/documentation/swiftui/view/chartxselection(value:) ; https://developer.apple.com/documentation/swiftui/view/chartangleselection(value:) ; https://developer.apple.com/documentation/swiftui/view/chartscrollableaxes(_:) ; https://developer.apple.com/documentation/swiftui/view/chartxvisibledomain(length:) ; https://developer.apple.com/documentation/swiftui/view/accessibilitychartdescriptor(_:) ; https://developer.apple.com/documentation/updates/swiftcharts ; https://developer.apple.com/documentation/accessibility/representing-chart-data-as-an-audio-graph ; https://developer.apple.com/videos/play/wwdc2024/10155/ ; https://developer.apple.com/videos/play/wwdc2022/110340/ ; https://developer.apple.com/forums/thread/845862 ; https://developer.apple.com/forums/thread/759270
- Windows: https://devblogs.microsoft.com/ifdef-windows/winui-gallery-3-0-is-here-tableview-charts-inkcanvas-and-more ; https://www.windowslatest.com/2026/10/31/one-reason-windows-11-has-so-many-web-apps-winui-still-lacked-basic-features-and-thats-finally-changing/ ; https://github.com/CommunityToolkit/Windows ; https://github.com/CommunityToolkit/Labs-Windows ; https://www.nuget.org/packages/LiveChartsCore.SkiaSharpView.WinUI ; https://github.com/beto-rodriguez/LiveCharts2/releases ; https://github.com/beto-rodriguez/LiveCharts2/issues/1513 ; https://github.com/beto-rodriguez/LiveCharts2/issues/1492 ; https://github.com/beto-rodriguez/LiveCharts2/issues/1076 ; https://www.nuget.org/packages/ScottPlot.WinUI ; https://scottplot.net/quickstart/winui/ ; https://scottplot.net/cookbook/5/ ; https://github.com/oxyplot/oxyplot ; https://github.com/HavenDV/H.OxyPlot ; https://www.nuget.org/packages/SkiaSharp.NativeAssets.Win32 ; https://www.nuget.org/packages/Microsoft.Graphics.Win2D ; https://www.syncfusion.com/winui-controls ; https://www.syncfusion.com/products/communitylicense ; https://www.telerik.com/winui/documentation/controls/radchart/chart-types ; https://www.telerik.com/winui/documentation/controls/radchart/behaviors/chartpanandzoombehavior ; https://www.devexpress.com/winui-free
- Declarative specs: https://vega.github.io/vega-lite/docs/encoding.html ; https://vega.github.io/vega-lite/docs/stack.html ; https://vega.github.io/vega-lite/docs/selection.html ; https://vega.github.io/vega-lite/docs/bind.html ; https://vega.github.io/vega/docs/transforms/treemap/ ; https://github.com/vega/vega-lite/issues/1872 ; https://github.com/vega/vega-lite/pull/9784 ; https://github.com/observablehq/plot (docs: marks, transforms/stack, transforms/bin, transforms/group, features/facets, marks/tree; issues 1224, 1710, 1732)
- Rust: https://docs.rs/plotters/latest/plotters/ ; https://docs.rs/plotters-backend/latest/plotters_backend/trait.DrawingBackend.html ; https://docs.rs/egui_plot/latest/egui_plot/ ; https://crates.io/crates/charts-rs ; https://crates.io/crates/treemap ; https://crates.io/crates/minmaxlttb ; https://crates.io/crates/lttb ; https://github.com/predict-idlab/tsdownsample
- Papers: Bruls, Huizing, van Wijk, "Squarified Treemaps", VisSym 2000, https://diglib.eg.org/handle/10.2312/VisSym.VisSym00.033-042 ; Jugel et al., "M4: A Visualization-Oriented Time Series Data Aggregation", PVLDB 7(10), 2014, https://datavis.cs.columbia.edu/files/papers/m4.pdf ; Steinarsson, "Downsampling Time Series for Visual Representation", University of Iceland, 2013, https://skemman.is/bitstream/1946/15343/3/SS_MSthesis.pdf ; Van Der Donckt et al., MinMaxLTTB, 2023, https://arxiv.org/abs/2305.00332

## 11. Capability review: every viewer, egui against the native shells (2026-11-24, overnight)

The maintainer, 2026-11-24: "one agent to pursue track 3, including view of tables (not just plots), and view of interchange formats (text/csv, text/svg). Review the plumcraft capabilities." Read on the dev branch at `a0ec42b`, after B154 slice 1 (view modes, histograms) and B159 (a pop-out's zoom kept through its items). A cell is **T** (a named test launched or drove it), **R** (read from the code), or **no**. Mac and iPad share `PlumCraftShell/Viewers.swift` except for the single picture; the iPhone is the iPad app.

### 11.1 The capability table

| Viewer | Capability | egui | Mac | iPad, iPhone | Windows |
|---|---|---|---|---|---|
| Picture (IMAGE, one array picture) | Fit, 1:1, zoom, channel, probe, pinned probe, detail on zoom | T | T | T | T |
| Picture | Zoom and view kept through a list's items | R (kept relative to fit) | T (B159) | T (B159, `testItemsKeepTheZoomAndView`) | no: every update refits (`Viewers.cs` `Apply`) |
| Array, several pictures | One shared zoom, pan and probe | R | T (slice 2: zoom asked through the core, both pictures at 200%) | T (slice 2, `testSliceTwoViews`) | no |
| Array | View modes (auto … angle) | T | T (B154) | T (B154) | R (B154 on Windows, built) |
| Array | Channel histograms, log scale, NaN counts | R | T (B154) | T (B154) | R |
| Array | 1-D series plot, table-of-positions plot | T (the core's list since slice 5; a series since slice 6, conformance) | T (slice 5; a series, slice 6: the core's line, reduced by M4) | T (slice 5; a series, slice 6, conformance) | no |
| **Table chart** | Column, stacked column, line, stacked line, histogram, treemap; columns chosen, kept with the graph | R (painted from the core's list, the core's spec; no choices of its own: `viewer.chart` from the control port) | T (slice 7) | T (slice 7, conformance) | no |
| Array grid | Paged, column window, plane | T | T | T | T |
| Array grid | Number format; colour by value | R | T (slice 2, conformance and launch) | T (slice 2, conformance) | no |
| **Table** (TABLE, rows, a table-flagged array) | Sort by a header | T (egui sorts in the UI) | T (the core sorts) | T | T |
| Table | Paged for millions of rows | no: every row read and laid out virtually from the whole JSON | T (500 a page) | T | T |
| Table | Row and column count | R | R | R | R |
| Table | Colour by value | R | T (slice 2) | T (slice 2, `testSliceTwoViews`) | no |
| Table | Column widths a person drags | R | R (SwiftUI `Table`) | no (fixed widths from the page) | no |
| Table | Select and copy a cell's text | R (egui labels select) | R (`textSelection`) | R (`textSelection`) | no |
| Table | Filter rows | no | no | no | no |
| Table | Number text the core's | no (egui's `to_string`) | T | T | T |
| Text (TEXT) | Wrap, monospaced, show all | R (wrap) | T | T | T |
| Text | Find, with a count | R | no | no | no |
| SVG | Drawn crisp at any zoom; source as text | T | T (picture, "Source") | T | T |
| JSON | YAML and outline | T | T | T | T |
| Audio | Waveform, spectrogram, play, seek, loop | T | T | T | T |
| Scalar | The value, large | T | T | T | T |
| A list | Stepper, named items, sheet | T | T | T | T |
| Bytes | Hex, paged | T | T | T | T |
| Compare | Side by side, split | T | T | T | T |
| Compare | Exact difference (mean, per channel) with probe | T | T (slice 2b; no probe on it yet) | T (slice 2b; no probe yet) | no |
| Mesh (STL) | Shaded, turned, zoomed | T | no | no | no |
| PDF | Pages | R | no (engine picture) | no | no |
| **`text/csv`** (TEXT holding CSV, or bytes typed `text/csv`) | As a table | R (slice 7: the registry test holds egui's offers to the core's; read by the shared reader) | T (slice 4, conformance and launch) | T (slice 4, conformance) | no |
| `text/csv` | As text | R (slice 7) | T (slice 4) | T (slice 4) | no |
| **SVG text** (TEXT holding SVG, or bytes typed `image/svg+xml`) | As a picture | R (slice 7, the registry test) | T (slice 4, launched; longer than its excerpt since slice 7, conformance) | T (slice 4, conformance) | no |

### 11.2 Tables as tables

What egui's table does that the native ones lack: **colour by value** (each numeric cell tinted from the column's minimum to its maximum, `viewers/basic.rs:160-215`), **dragging a column's width** on the iPad and Windows, and **selecting a cell's text** on Windows. What the native ones do that egui doesn't: the core reads, sorts and pages the table on its workers (egui reads all of it, on the UI thread's frame, so a million rows is slow), and number text is the core's. Neither filters rows; the maintainer asked for "selecting columns, rows" (note 26 slices 8 and 9). So the table regressions are colour by value (all three native shells), column widths on the iPad and Windows, and cell copy on Windows.

### 11.3 Interchange formats: `text/csv` and SVG as text

How a value gets one of these media types today:

- **Bytes typed by a media type.** The text engine's file types `CSV`, `TXT`, `MD`, `HTML` (`crates/text/src/files.rs:44`, for the craft packs' exports) are blobs with `meta.media` `text/csv` and so on, `Media::Bytes`. Every shell offers only `hex` (egui adds Open in another app). An SVG blob is `Media::Vector` already (`engine/src/exec.rs:1213`) and shows as a picture everywhere.
- **TEXT holding CSV or SVG.** A TEXT value's media type is always `text/plain; charset=utf-8` (`docs/interface.md` section 3, the TEXT row); what the text says is not declared. Table to Text writes CSV as TEXT; a Text node can hold an SVG. Every shell shows them as text.
- **A file read by name.** `plumcraft_proto::media::media_for_ext` maps `.csv` to `text/csv` and `.svg` to `image/svg+xml` (`crates/proto/src/media.rs`).

What `docs/interface.md` says: `DataRef.meta.media` is the value's media type (`crates/proto/src/data.rs:41`, "`application/x-npy`, `text/plain; charset=utf-8`, `image/png`, …"), each core type has one fixed media type (section 3's table), and a pack type's blob carries its own. So the media type is already on every full value; **the mapping from a media type to the viewers offered belongs in the core's registry** (`plumcraft_editor_core::viewers::offered`, held to egui's by a test), as XML's already does (`is_xml`, from `meta.media`). The reading of CSV for the table viewer belongs beside the core's other value readers (`plumcraft_media::table`, which reads TABLE JSON and rows, and since B152 a table-flagged array): a display reader, as decoding a PNG is, not an engine node, so AGENTS.md rule 3 (no engine in core) holds. The text engine's CSV reader (`crates/text/src/files.rs:308`, quotes, line ends, typed columns) moves into `plumcraft-proto` so the engine and the viewer read CSV the same way.

**Assumed for the maintainer** (see 12.2): a value typed `text/csv` (bytes, or a TEXT whose file was `.csv`) is offered **Table first, then Text**; a value typed `image/svg+xml` that is TEXT is offered **Picture first, then Source**. The choice is the existing View As (`view.viewer {popout, viewer}`), already a core command kept in the pop-out's state, so no new command is needed. A TEXT whose content merely looks like CSV is not sniffed: guessing is left out (a sniffed value could be offered "Table" as a second choice later).

## 12. The plan, revised (overnight, 2026-11-24)

### 12.1 Slices

Regressions first (decision 2A); a table or interchange view egui has counts as a regression.

| Slice | What | Gate | Launch |
|---|---|---|---|
| 1 (done) | View modes, histograms, NaN counts | Conformance "an array's view is chosen through the core and its pictures change" | Mac, iPad, iPhone (B154) |
| 2 and 2b (built) | **Pictures and grids**: the several-picture view gets one shared zoom and pan (`PictureZoom`, B159) and the probe; colour by value and number format in the array grid and colour by value in the table (`viewer.grid {colored, format}`, `viewer.table {colored}`, the page carrying each cell's 0-to-1 position); Compare's exact difference as the core's `derive:compare` picture; B161 (the iPhone's viewer bar overflowing) | Conformance cases per command; Swift unit tests | Mac scripted copy and the simulators, each view pictured |
| 3 | **Tables as tables**: cell text selectable and copied (Windows), column widths a person drags (iPad, Windows), the row and column count in the bar | UI test | iPad |
| 4 (built) | **Interchange formats**: `text/csv` offered Table then Text, SVG text offered Picture then Source; the CSV reader shared (`plumcraft_proto::csv`), in the hostile sweep, with size limits | Conformance "a CSV value is a table a viewer away from its text"; hostile sweep | Mac, iPad |
| 5 (built) | **The plot again, on the chart engine** (was 3): display list, scales, ticks; table of positions, B152's histogram as columns | Display-list fixtures in Rust, Swift, C# | Mac, iPad |
| 6 (built, part) | **1-D series** (was 4): a 1-D array is `array.plot`, a line by index reduced by M4 to four points a pixel column. **Hover previews deferred:** a node's face and a wire's hover show the engine's thumbnail (`Rendition`), and the core has no picture of its own to put there; drawing one needs the chart's list made into a picture on the media workers (an SVG of the list, rasterised as SVG values are) and the scene asking for it, a slice of its own. Audio's (frames, channels) as lines waits with it: a 2-D array is a picture today | Conformance "a 1-D array is a line by index, reduced to the plot's width"; unit tests of the reduction (every spike kept) | Mac |
| 7 (built) | **The maintainer's six kinds, with column choice** (were 5 to 7): `table.chart` offered after Table; `viewer.chart {popout, mark, x, y, color, size, path, agg, stack, bins, clear}` checked against the table's columns and types and kept in the graph's `meta.charts` (decision 3A); column, stacked column, line, stacked line, histogram and treemap (squarified) laid out by the core into the same list, with colours, legend, areas and labels; the Apple shells' choices as menus; egui paints the list and offers what the core does for CSV and SVG text; SVG text longer than its excerpt drawn from its bytes (both SDKs now send the whole text when the excerpt doesn't hold it) | Conformance "a table's chart is chosen by command, kept in the graph and drawn from the core's aggregate" (every kind, the refusals, undo, clear); the long-SVG steps; unit tests per kind (treemap areas within 1e-6 of their shares) | Mac |
| 8 (built) | **Time axes, shares, zoom, selection, readouts**: ISO dates and times (or `DATE`, `TIME`, `TIMESTAMP` columns) as a time axis ticked by the calendar; `normalize` for a stacked chart's shares; `viewer.range` (the pop-out's zoom and pan, the line reduced again within it, marks clipped to the plot); `viewer.select` (a brush of a column, or rows; the selection the value's, decision 7A, so `view.popout {another: true}` opens its table beside its chart, which marks the rows and can show only them); `viewer.data {part: "hit"}` (the readout under a point, the core's); Compare's difference probe (part 1). Apple: drag to brush, pinch to zoom, hover (Mac) or tap (iPad) to read, Reset Zoom, Clear Selection, Open Table; egui draws the range and selection; Windows drag, wheel and hover (built) | Conformance "a chart's range and the value's selection are the core's, and so is what is under a point"; unit tests of the time parser (in the hostile sweep), ticks, shares, range, selection masks and hits; iPad UI test brushing a histogram | Mac |
| 9 (built) | **Face and hover previews** (deferred from 6; the maintainer's option A, 12.3): the chart layout moved to its own crate, `plumcraft-chart`, shared by the core and the Rust engine SDK; a rasteriser for the display list (`raster`, tiny-skia); the Rust SDK's thumbnail of an array the core plots is that chart | Conformance "a chart's face shows the chart the core lays out"; unit tests of the rasteriser and the SDK's thumbnails (pixel probes); the crate's hostile sweep | Mac |
| 10 | Optional kinds | As before | As before |

### 12.2 Assumed (the controller's instruction, the maintainer away overnight)

**Answered.** The maintainer, 2026-11-25, on the checklist page of 29 points (cards 1 to 15 are N1 to N15 here, 16 and 17 the release and merge points R1 and R2, 18 to 29 are V1 to V12 of note 26): "Decisions: 1A; 2A; 3A; 4A; 5A; 6A; 7A; 8A; 9A; 10A; 11A; 12A; 13A; 14A; 15A; 16A; 17A; 18A; 19A; 20A; 21A; 22A; 23A; 24A; 25A; 26A; 27A; 28A; 29B". So items 1 to 11 below stand as built (cards 18 to 28). Item 12 is changed (29B): the editor asks the engine for the rest of a long TEXT, a new message, in place of the engine sending the whole text. Then, the same day: "Stop. Take the long text". So item 12 stands as built too (29A): the engine sends the whole of a long TEXT.

The maintainer: "Make reasonable assumptions about what I choose if I am not available for a decision". Each with the option taken first and the alternative.

1. **CSV as a table:** a value typed `text/csv` is offered Table, then Text (View As). Alternative: Text first, Table second.
2. **SVG text as a picture:** a TEXT typed or named as SVG is offered Picture, then Source. Alternative: Source first.
3. **No sniffing of CSV:** only the declared media type makes a value a table; text that looks like CSV is not offered one. SVG is the exception, since a TEXT can't be typed and an `<svg` root element is unambiguous: a TEXT starting with one is offered the picture (as built in slice 4). Alternative: sniff CSV's first lines too and offer Table second.
4. **The choice is View As** (`view.viewer`), not a new view-mode command like B154's `run.view`, because the reading is the core's and needs no engine run. Alternative: a `viewer.format` command.
5. **Where CSV is read:** a shared reader in `plumcraft-proto`, used by the text engine and by `plumcraft-media` for display. Alternative: the viewer runs the text engine's Text to Table through Convert (an engine round trip for every look).
6. **Colour by value** is off by default in the grid and the table, as in egui (it is a checkbox there, off until ticked). Alternative: on for numeric columns.
7. **Shared zoom for several pictures** keeps one `PictureZoom` for all the value's pictures, in the shell, as B159 keeps a single picture's. Alternative: move the zoom into the core now (note 26 slice 2 as first written).
8. **Compare's difference** is drawn from the core's `derive:compare` picture, as the contract (13.2) says, not computed in the shells. Alternative: each shell computes it.
9. **Chart after Table** (slice 7): a TABLE, CSV or rows is offered Table first and Chart second; a table array the core plots keeps Plot first and gets no Chart beside it (one chart a value). Alternative: Chart first for tables with a number column.
10. **The chart picked with no choice:** a column chart of the first measure (a column with fractions before one of whole numbers, which are often counts, months or codes) summed by the first category column; a histogram when there is no category. Alternative: always a histogram of the first number.
11. **The spec's names** are note 26's (`mark`, `x`, `y`, `color`, `size`, `path`, `agg`, `stack`, `bins`); a stacked line is `line` with `stack`, a stacked column `column` with `stack`, so the six kinds are four marks. Time axes, `normalize` and `range` wait for slice 8. Alternative: six marks by name.
12. **The whole of a long TEXT** goes to the editor when its excerpt doesn't hold it (both SDKs, `docs/interface.md` section 10, a SHOULD): before, a TEXT between 4096 characters and the inline limit reached the editor only as its first 4096, so an SVG of that size could never be drawn. Alternative: the editor asks the engine for the rest (a new message).

### 12.3 Face and hover previews: the decision and what was built (2026-11-25)

Slice 6 deferred them: a face shows the thumbnail its engine sends, and the core holds only the values something watches, so it can't draw a face's chart itself. The options put to the maintainer:

- **A.** Move the chart layout into a crate the Rust engine SDK can use too, add a rasteriser for the display list, and have the SDK send a chart thumbnail for a 1-D array or a plotted table: faces show the picture the core lays out. Python engines would need their own, or get it later.
- **B.** The core watches the first output of every face without a thumbnail (a watch per node, against the run model).
- **C.** Each SDK draws a simple line picture of its own, not matching the core's chart.

The maintainer, 2026-11-25: **"Chart: a"**.

What was built:

- **`plumcraft-chart`, a new small crate**, not the protocol crate behind a feature: the protocol crate is the contract every engine and shell links, and a chart's layout and drawing are not part of the contract; a crate of its own keeps tiny-skia out of everything that doesn't draw. It holds the display list's layout for arrays (`layout`: kinds, scales, nice ticks, M4 reduction, a 1-D series, the part shown and the selected rows), the core's number text (`number`), the array reading it needs (`dtype`), the span types of `viewer.range`, and behind the `raster` feature a rasteriser for any display list (tiny-skia, pure Rust; no text drawn; every size bounded, junk skipped; its own hostile sweep, `crates/chart/tests/hostile.rs`). It depends on the protocol crate, serde and, with `raster`, tiny-skia: no GUI toolkit and no engine, so `plumcraft-editor-core` and `plumcraft-media` still build none (golden rule 3). The core uses it without `raster`; the SDK with it.
- **Not moved:** the table chart kinds (`chart_kinds`: column, histogram, line and treemap of a TABLE's columns) stay in the core, since they read a TABLE through `plumcraft-media`'s table reader, which the SDK doesn't need for a face: a face's chart is an array's (a 1-D array, or a table array the core plots: points, segments, a histogram's columns).
- **The Rust SDK** (`arrays::chart_thumb`): the thumbnail of an array the core plots is its chart, laid out at the core's default size, its plot drawn into the face's thumbnail (160 by 106 pixels: the node thumbnail's long side, `THUMB_PX`, as before), the chart's kind its rendition's `caption` (`line`, `columns`, `points`, `segments`). The message is the existing `rgba8` thumbnail rendition: no protocol change.
- **Hover:** the egui editor's wire hover shows a watched value's viewer preview, which for an array the core plots is the same layout painted (`viewers::plot`, slice 5); its faces show the SDK's thumbnail. The Apple shells show the thumbnail on faces (the scene's preview, the engine's rendition) and have no wire or port hover preview of a value; Windows likewise shows the face's thumbnail.
- **The Python SDK has not got it.** Its engines still send no chart thumbnail: nothing for a 1-D array, and a table array's picture as before. It would get it either as a wheel of `plumcraft-chart` (a small PyO3 module exposing `layout` and `raster::draw`, built beside the engine SDK's wheels), or by a port of the array layout of its own (a few hundred lines of numpy, held to the Rust one by shared fixtures of display lists). The wheel is the one that can't drift.
