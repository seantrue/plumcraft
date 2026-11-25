# 30. Views from every port: the full range of looking, computed in the engine base classes

**Research, 2026-11-25, for FR6. No code. Read on `feat/native-n0` at `63e9aad`.**

The maintainer, 2026-11-25, in an application built on the Mac shell, looking at the array output of one of its nodes:

- "I get an array with only picture/value/bytes. No charts or table options."
- (told a chart needs tabular data, and offered a histogram node:) "No."
- "Exploratory data visualization should not require altering the graph. For each port, I should have the full range of visualizations available directly."
- "Review the original brief for visualization. There was a lot agreed on.Let's take an instance. We have nodes that create labeled maps (each pixel has an assignment to a region). I expect that creating a treemap from the labeled map should be natural. Taking a histogram of 3d data should be as natural as 1/2d."
- "My inclination would be to provide all of this functionality in engine base classes, so that it is only implemented a few times (maybe just twice: once for rust, once for python)."

## 1. Summary

- **The maintainer is right that much was agreed and is not in the app.** The protocol has specified derivatives on request since protocol 2 (`docs/interface.md` 10.4 `want`, 10.5 `blob.derive`), the N-D slice picker (10.1: "N-D → slice picker"), and "a preview and full popout window for every base type" (note 16). None of these was built for arrays beyond rank 3. The six chart kinds (note 26 slice 7) were built **for TABLE values only**. A picture, a 3-D volume or a labelled map gets Pictures, Values and Bytes, which is exactly what the maintainer saw.
- **The gate is in one function.** `viewers::offered` (`crates/editor-core/src/viewers.rs:117`) offers Chart only to a TABLE, CSV or rows value, and Plot only to an array an engine flagged `table: true` with recognised columns or to a 1-D array (`plumcraft_chart::plotted`, `crates/chart/src/layout.rs:68`). Every other array, whatever its rank, has no chart, no histogram view and no treemap.
- **Derived data today requires a node.** A histogram of anything but the last axis's channels, a region table of a labelled map, or a statistic per plane each need a node in the graph. That contradicts "looking never edits the graph".
- **The architecture the maintainer proposes already has a precedent in the SDKs.** The per-channel 256-bin histograms in every array's `stats` are computed by both SDK base classes, Rust (`crates/engine/src/arrays.rs:313`) and Python (`python/src/plumcraft_engine/arrays.py:245`). So every engine on either SDK already gets one reduction for nothing. FR6 extends that precedent from "one fixed reduction at production time" to "any reduction, on request, of a value the engine holds".
- **Recommended design:**
  - **Reductions in the SDK bases.** A new additive capability and op, `view.derive {ref, view, max}`, answers a small typed value (a TABLE, a reduced ARRAY) with a handle of its own. Its identity is `hash(source handle, canonical view spec, deriver version)`, so it is cached, popped out, brushed and compared like any value.
  - **Implemented twice, fallback included.** The Rust reductions live in a new GUI-free crate, `plumcraft-views`, used by the Rust SDK and also compiled into the editor as a built-in views engine for any engine without the capability. The Python port is held to the Rust one by a shared vector file.
  - **Layout stays where it is.** The core keeps laying charts out (`plumcraft-chart`, note 26 decision 1A). `offered` offers by value kind and rank, never by a flag; a flag only picks the default.
- **The two instances, end to end** (section 3.3):
  - **A labelled map:** View As › Treemap. The engine counts the regions into a TABLE (label, area, bounding box, centroid). The core lays that out as a squarified treemap and every shell paints it.
  - **A 3-D array:** View As › Histogram. The engine bins all its values (or per plane along a chosen axis) into a TABLE of `centre`, `count`, which the core draws as the existing histogram chart.
- **Fourteen decisions** are in section 8, all decided A ("All A", 2026-11-25), with the principle "In general, it's the user's job to deal with odd, and your job to make seeing odd things easy."

## 2. The original brief, reviewed

### 2.1 The brief in the maintainer's words, and the decisions taken on it

| When | Words (verbatim) | Where recorded |
|---|---|---|
| 2026-10-30 | "We need a preview and full popout window for every base type, and an architecture to make it easy to add new ones .. check jetbrains for an array display." | Note 16 |
| 2026-10-30 | "Popout previews should not block interaction in the editor, and updates should update the popout display." | Note 16 |
| 2026-10-30 | "Add preview for data on a wire." | Note 16 |
| 2026-10-30 | "Look at egui_extras for table and svg views" | Note 16 section 3b |
| 2026-10-30 | "Rows should be visualized with the table viewer" | Note 16 section 3a |
| 2026-11-24 (B154) | "regression in views of arrays (the egui version supported plots, view as image, heatmap, etc). These are important. For tabular data, visualizing selected columns is also important." | Note 26, BUGS B154 |
| 2026-11-24 (B154) | "B154: Start this as a parallel research task. Survey first. Look at seaborn. Histograms. Line charts, stacked line charts, column charts, stacked column charts,  treemaps. Interactive is good (selecting columns, rows, etc; for interrogating data, zooming into ranges)" | Note 26, BUGS B154 |
| 2026-11-24 | "Yes to all" (note 26 decisions 1 to 7: core lays out, shells paint; regressions first; chart choices saved with the graph; egui paints the same list; Win2D; the ranking of optional kinds; a selection belongs to the value) | Note 26 section 8 |
| 2026-11-24 | "one agent to pursue track 3, including view of tables (not just plots), and view of interchange formats (text/csv, text/svg). Review the plumcraft capabilities." | Note 26 section 11 |
| 2026-11-25 | "Decisions: 1A; … 28A; 29B", then "Stop. Take the long text" (so 29A: the engine sends the whole of a long TEXT) | Note 26 section 12.2 |
| 2026-11-25 | "Chart: a" (the chart layout in a crate the Rust SDK shares; face thumbnails drawn by the SDK) | Note 26 section 12.3 |
| Stage 5 of the application | A signals view: a list of named pictures, each plane with its own mapping | The application's own notes (private, not read for this note) |

Under these sit the floor rows of the research software's viewer (note 04 section 2, `docs/plan.md` F50 to F57). There is also the protocol-2 design that the maintainer accepted when the interface was written (note 04 sections 3 to 9, `docs/interface.md` 10).

### 2.2 Agreed and built

| What | Built in | Where |
|---|---|---|
| A pop-out for every value, not modal, following its port, updated by every run | Note 16 steps 1 to 4; native shells N4 slice 5 | `crates/session/src/view_cmds.rs`; `PlumCraftShell/Viewers.swift`; `native/windows/.../Popouts.cs` |
| Picture: fit, 1:1, zoom, channel, probe, pinned probe | Note 16; N4 | Every shell (note 26 row 1) |
| View modes (auto, colour, stretch, clip, heat, labels, angle with turn) | B154 slice 1 | `popout.get` `viewModes`; `run.view`; every native shell (Windows on `feat/b154-s1-windows`) |
| Per-channel histograms (256 bins), log toggle, NaN counts | B154 slice 1 | The SDKs' `stats` (`crates/engine/src/arrays.rs:313`, `python/.../arrays.py:245`); drawn by every shell |
| Several pictures sharing zoom, pan and probe | Note 26 slice 2 | `PictureZoom`, B159 |
| The numbers grid: paged, a column window, one plane index | N4 slice 5; note 26 slice 2 (format, colour by value) | `viewers.rs` `data`; every shell |
| Table: sorted and paged by the core, colour by value, column widths | N4 slice 5; note 26 slices 2 and 3 | `plumcraft-media` table reader |
| Compare side by side and split; the exact difference | N4; note 26 slice 2b | `derive:compare` |
| CSV as a table, SVG text as a picture | Note 26 slice 4 | `plumcraft_proto::csv`; `viewers::svg_text_ref` |
| The plot as the core's display list | Note 26 slice 5 | `crates/chart`; painters in every shell |
| A 1-D array as a line, reduced by M4 | Note 26 slice 6 | `plumcraft_chart::layout` `Kind::Series` |
| The six chart kinds with column choice, kept in `meta.charts`, undoable | Note 26 slice 7 | `crates/editor-core/src/chart_kinds.rs` |
| Time axes, shares, zoom (`viewer.range`), selection shared by a value's windows (`viewer.select`), readouts | Note 26 slice 8 | Conformance "a chart's range and the value's selection are the core's …" |
| Chart face thumbnails | Note 26 slice 9 (the maintainer's "Chart: a") | `plumcraft-chart` `raster`; Rust SDK `arrays::chart_thumb` |
| The whole of a long TEXT sent (29A) | Note 26 slice 7 | Both SDKs; `docs/interface.md` 10.2 |
| Audio: waveform, spectrogram, play | N4 slice 5 | `plumcraft-media` `Want::Waveform`, `Want::Spectrogram` |

### 2.3 Agreed and not built, or built narrower than agreed

Numbered for reference in the decisions. "Agreed" means the maintainer said it, decided it on a checklist, or accepted it as part of the interface design.

1. **Derivatives on request.** `docs/interface.md` 10.4 (`want: [{kind: "peaks"}, {kind: "spectrogram"}, …]` in every WatchRequest) and 10.5 (`blob.derive{handle, spec}`, capability `blob.derive`, specs `thumb`, `preview`, `tile`, `region`, `probe`, `diff`, `peaks`, `frames`, `lines`, `raster`). **Neither SDK implements either.** `blob.derive` exists only as a capability constant (`crates/proto/src/caps.rs:29`), and no engine reads `want`. The derivative machinery the maintainer's inclination calls for was designed, accepted and never built.
2. **The N-D array dispatcher.** Interface 10.1: "N-D → slice picker". Note 04 section 7: "N-D → axis pickers for the two image axes and sliders for the rest, then Image". **What was built:**
   - The numbers grid has one flattened plane index.
   - Pictures are made only for rank 2 or rank 3 with at most 64 planes on the last axis (`arrays::layout`, `crates/engine/src/arrays.rs:39`).
   - A (D, H, W) volume is read as H×W pictures with **W channels**, so its axes are wrong. It gets no picture at all when W > 64.
   - A rank-4 array gets no picture.
   - There is no axis picker.
3. **Histograms, narrower than "Histograms":**
   - A histogram exists only per channel of the last axis (at most 64 channels), always 256 bins over each channel's own range.
   - There is no choice of bins or range, no histogram over all values of a multi-channel array, none per plane along another axis, and no 2-D joint histogram.
   - The table chart's histogram (slice 7) bins a TABLE column only.
4. **Charts, narrower than the brief.** The brief's list (histograms, line, stacked line, column, stacked column, treemap) was built for TABLE, CSV and rows values only. An array gets Plot only if an engine flagged it a table with recognised columns, or if it is 1-D (`plotted`). Even then it gets **no Chart beside its Plot**: note 26 section 12.2 item 9, an assumption made while the maintainer was away, later accepted in the "1A … 28A" answer, says "one chart a value". So for a picture, a plane or a volume, no chart kind is reachable.
5. **Treemap, narrower:** it is built from TABLE category columns only. A labelled map can't reach it without a node that makes a table of regions, and no such node exists in this repository. The array engine's Connected Components makes the labelled map; nothing counts its regions.
6. **The optional chart kinds** (decision 6A: grouped columns and scatter "with the required kinds"; heatmap of a pivot, ECDF and box next; KDE and violin last). **None is built**, grouped columns included (`chart_kinds.rs` has no `dodge`).
7. **The Python SDK's chart thumbnails** (decision "Chart: a", note 26 12.3). Built for Rust only. Python engines' 1-D arrays have no face picture and their plotted tables show the old picture.
8. **Note 04's viewer table** (accepted as the plan for protocol 2):
   - Freeze pins (compare by value: this run against the last), and Blink and Overlay compare. Not built; compare is by request only.
   - Text find (egui only) and text diff. Not built natively.
   - Table row and cell diff. Not built.
   - A scalar's history sparkline over recent runs. Not built.
   - SVG element outline on hover. Not built.
   - The Composite viewer (fields side by side) and the Sequence scrubber. Not built in any shell.
   - The mesh viewer: egui only.
9. **Previews on wires** (note 16, "Add preview for data on a wire"): egui only. The Apple shells have no wire or port hover preview of a value (note 26 12.3).
10. **"Selecting … rows":** a brush and `selectedOnly` were built; **filtering** rows was not.
11. **"For each port":** a pop-out follows an **output** port. An input has no "show this" of its own: a wired input must be read at its source, and an unwired input's constant shows only as its widget.

### 2.4 Where what was built contradicts the principle now stated

1. **`viewers::offered` gates views on facts only a node sets.** Plot needs `table: true` (or rank 1); Chart needs a TABLE. The value's shape and dtype already say what can be shown; a flag should choose the default, not the menu.
2. **"One chart a value"** (12.2 item 9) closes Chart to every plotted array. It was a choice of defaults, made while the maintainer was away, that became a gate.
3. **Derived data requires a node.** My own answer in the conversation that raised FR6 ("add a histogram node") follows from it. A histogram along an axis, a region table, a statistic per plane, a profile along a line and the spectrum of a sound are each a node to add and a wire to make, which is altering the graph to look.
4. **The SDKs are not equal.** Chart face thumbnails come only from the Rust SDK. The per-channel histogram is the only reduction both do, and nothing checks that they agree (no shared vector covers `stats`).
5. **Aggregation for charts is in the editor** (`chart_kinds.rs` over `plumcraft-media`'s table reader). That was note 26's decision 1A (option B), and it is right for layout. For the reductions FR6 needs, it means the bytes must come to the editor, which is the wrong place for a remote engine, a 4 GB volume or a value kept for nobody (section 5).
6. **The derivative design (2.3 item 1) was replaced in practice by editor-side derivation.** The editor's media service (`crates/media/src/service.rs:69`, `Want::Waveform`, `Spectrogram`, `Sorted`, `Table`) computes what interface 10.5 said engines would offer. It works on one machine with the store mapped; it is not the design that was agreed for remote engines.

## 3. The principle, and what follows

### 3.1 The principle

For every port of every node, every view that suits the value's kind and shape is offered directly, in the value's pop-out (View As) and its context menu, and **looking never edits the graph**. Derived views are not nodes, add no wires and change no result hash.

**Inputs too** (decision 7):

- A wired input shows the value it receives: the upstream output, sliced as the wire slices it.
- An unwired input shows its own value: its constant or default, as a value.
- An input is reached from its row's context menu (View Value…), as an output is from its pin.

### 3.2 The full range: the matrix

**Value kinds** are told apart by media kind, dtype and shape, never by a flag. **Key:**

- **B** built today;
- **N** narrower than this row asks;
- **S** a reduction in the SDK base (new);
- **C** laid out by the core from a small result (existing `plumcraft-chart`, or new where marked);
- **–** not meaningful.

| View ↓ / value → | Numeric array, rank 1 | Rank 2 (H, W) | Rank 3 picture (H, W, C ≤ 64) | Rank ≥ 3 volume or stack (any axes) | Integer labelled map | Table (TABLE, CSV, rows, flagged array) | List, nested list | Audio | Text | JSON | SVG |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Picture / planes / slices | – | B | B | **S** slice along any two axes, sliders for the rest | B (`labels` view) | – | B (item stepper, sheet) | B (spectrogram picture) | – | – | B |
| Values grid | B | B | B | N (one plane index) → axis picker | B | B | per item | – | – | B (tree) | – |
| Table | **S** index, value | **S** row, column, value (or columns as fields) | **S** pixels × channels | **S** flattened with index columns | Regions table (below) | B | **S** one row per item (summary, stats) | **S** frames × channels | – | **S** rows from an array of objects | – |
| Statistics (min, max, mean, sd, NaN, ±inf, percentiles) | N (min, max, NaN only) | N | N | **S** over all, per plane along a chosen axis | **S** per region | B (column stats) / **S** percentiles | **S** per item | **S** per channel, RMS, peak dB | **S** chars, words, lines | – | – |
| Histogram: over all values | **S** + C | **S** + C | **S** + C | **S** + C | **S** counts per label (= areas) | B (TABLE column) | **S** over all items | **S** sample histogram | – | – | – |
| Histogram: per channel / plane / axis | – | – | N → **S** bins and range chosen | **S** per plane along a chosen axis, small multiples or stacked | – | B (colour by category) | **S** per item | **S** per channel | – | – | – |
| Histogram: 2-D joint of two channels or planes | – | – | **S** + C (a heatmap) | **S** + C | – | **S** two columns | – | – | – | – | – |
| Profile / line along an axis | B (the series) | **S** a row or column, or a drawn line (with the probe) | **S** per channel | **S** along any axis through a point; mean along an axis | – | B (line chart) | **S** a statistic across items | B (waveform) | – | – | – |
| Scatter of planes / channels | – | – | **S** + C (density past a few thousand points) | **S** + C | – | 6A, not built | – | **S** two channels (Lissajous) | – | – | – |
| Chart kinds (column, stacked, line, stacked line, histogram, treemap) | via the Table row | via the Table row | via the Table row | via the Table row | via the Regions table | B | via the Table row | via the Table row | – | via the Table row | – |
| Treemap | – | – | – | – | **S** regions + **C** (area, colour by a second port's mean) | B (category columns) | **S** items by size | – | – | **S** keys by size | – |
| Region table (label, area, bbox, centroid, perimeter; mean, sd of a second port) | – | – | – | – | **S** | – | – | – | – | – | – |
| Spectrum | **S** (FFT of the series) | **S** 2-D magnitude, radial profile | **S** per channel | – | – | – | – | **S** + C (average spectrum; spectrogram B) | – | – | – |
| Text views | – | – | – | – | – | B (CSV as text) | – | – | B (wrap); find N | B (YAML, outline) | B (source) |

Notes on the matrix:

- **"Via the Table row":** once any value has a table view, every chart kind of note 26 is offered on it. So the chart kinds come to arrays by making a table of them (a small one, made by the SDK), not by a second chart engine.
- **"Statistics" N:** today `stats` carries min, max and NaN per channel; mean, standard deviation, percentiles and ±inf counts are new.
- **The labelled map's "second port":** the treemap and the region table can colour or summarise each region by another value of the same size (the picture the map was made from, the usual case). It is chosen in the view, from the node's own inputs and outputs of the same height and width: a stretch goal (decision 9).

### 3.3 The two instances, end to end

**A labelled map → treemap.** One of the application's nodes makes a labelled map: (H, W) `<i4`, each pixel a region number. The array engine's Connected Components does the same in public code (`crates/array/src/generate/regions.rs`).

1. The person double-clicks the output's wire, or picks the node's menu › Outputs › labels › Open. The pop-out shows Pictures in the `labels` view, as today.
2. View As now lists Pictures, Values, **Regions**, **Treemap**, **Histogram**, Statistics, Bytes. Regions and Treemap are offered because the dtype is integer and the rank is 2 (or 3, a labelled volume): the matrix's row, not a flag.
3. The person picks Treemap. The core sends the engine that made the value `view.derive {ref: <the labels' DataRef>, view: {kind: "regions", params: {}}, max: {rows: 100000}}`. The SDK base, not the node, reads the array from the store (mapped, same host) and counts each label's pixels, bounding box and centroid in one pass. It writes a TABLE (`label` INT, `area` INT, `x0 y0 x1 y1` INT, `cx cy` FLOAT), stores it, and answers its Display. The handle is `hash(labels handle, {kind: regions}, deriver version)`, so a second look, a rerun with the same labels, or another window costs nothing.
4. The core lays the table out as a treemap: path `label`, size `area`, colour by area or by label. This uses `plumcraft-chart`, the treemap moved there from `chart_kinds.rs` (decision 10). The shell paints the display list as it paints every chart today.
5. **Hover** gives the label, its area in pixels and its share. A **click** selects the region. The selection belongs to the value (note 26 decision 7A), so a Pictures window of the same port shows that region's pixels highlighted (a mask the core asks the SDK for, `view.derive {kind: "select-mask", params: {labels: [7]}}`), and the Regions table shows its row.
6. **Stretch:** in the treemap's bar, "Colour by: mean of <picture>". The SDK's regions reduction then takes a second ref and adds `mean`/`sd` columns per region.
7. **Thousands of regions:** the treemap shows the largest up to the plot's capacity (about one region per 64 square points) and an "other" cell. The table keeps every row, paged by the core as tables already are.

**A histogram of a 3-D array.** A node outputs (D, H, W) `<f4`, a volume.

1. Pop it out. Today it shows W pictures of D × H, or none if W > 64. After FR6, Pictures shows one slice with two axis pickers (default: the last two axes, H × W) and a slider for D (2.3 item 2). The SDK renders the slice the view names.
2. View As › **Histogram**. The core sends `view.derive {ref, view: {kind: "histogram", params: {bins: 256, over: "all"}}}`. The SDK bins every finite value in one streaming pass over the mapped store file, in f64, and counts NaN and ±inf apart. It answers a TABLE (`centre` FLOAT, `count` INT, plus `nan`, `posinf`, `neginf` in the table's meta), which the core draws with the existing histogram chart (`chart_kinds.rs` histogram of a binned table: `centre`, `count`).
3. In the bar:
   - Bins (16 to 65,536) and Range (finite min..max, or typed);
   - Log;
   - **Per**: all, or per plane along axis 0, 1 or 2. Per plane answers a TABLE with a `plane` column; the core draws it stacked, or as small multiples once faceting exists, or as a heatmap of plane × bin, the cheap one.
   - Each choice is a command (`viewer.histogram {popout, bins?, range?, per?, log?}`), its state in `popout.get` and kept in the graph's `meta` like `meta.charts` (decision 6), outside the result hash.
4. **2-D joint:** "Joint with…" picks a second plane (or a second array of the same shape). The SDK answers a 2-D count array, drawn as a heatmap picture with axes.
5. A 1-D array, a 2-D plane and a 3-D picture take the same path. That is "as natural as 1/2d": one view, one reduction, any rank.

### 3.4 What the views are, in protocol terms

| View kind (`view.kind`) | Params (all optional) | Answer |
|---|---|---|
| `stats` | `per`: `all` or an axis; `percentiles` | TABLE: one row per plane (or one), columns min, max, mean, sd, nan, posinf, neginf, p… |
| `histogram` | `bins`, `lo`, `hi`, `per`, `log` (display only) | TABLE `centre`, `count` (+ `plane`) |
| `histogram2d` | `a`, `b` (plane indices or a second ref), `bins` | ARRAY (bins, bins) `<u8` counts |
| `profile` | `axis`, `at` (an index tuple), or `line: [[r0, c0], [r1, c1]]`, `reduce` (`none`, `mean`, `max`) | TABLE `t`, `value` (+ channels) |
| `slice` | `axes: [i, j]`, `at` (indices for the rest), `px` | ARRAY (h, w[, c]), then rendered as pictures by the existing view modes |
| `table` | `max_rows` | TABLE (index columns + value columns) |
| `regions` | `with` (a second ref), `max_regions` | TABLE label, area, bbox, centroid (+ mean, sd of `with`) |
| `select-mask` | `labels`, or `brush` (a value range) | ARRAY (H, W) `|u1` mask |
| `spectrum` | `axis`, `window`, `n` | TABLE `frequency`, `magnitude` (+ channel) |
| `scatter` | `a`, `b`, `max_points` | TABLE `x`, `y` (sampled), or ARRAY density when past `max_points` |

Every answer is a value the editor already knows how to show. So a derived view needs no new viewer: Table, Chart (the note 26 kinds) and Pictures draw it. The work is the reductions and the offer, not new drawing.

## 4. Design: the functionality in the engine base classes

### 4.1 The interface addition (`docs/interface.md`)

- **Capability `view.derive`** (5.3): "The engine answers `view.derive`; derived views of values it holds". If it is absent, the editor's built-in views engine answers when it can read the value (4.5), else the view is offered disabled with the reason.
- **Op `view.derive`** (6.1):
  - **Params:** `{ref: DataRef, view: {kind, params}, with?: [DataRef], max?: {bytes, rows, cells, ms}, id}`.
  - **Result:** `{display: Display}` (the derived value, `full` its DataRef) or `{error}`.
  - **Timeout:** 30 s to an ack, then events.
  - **Long work:** a request expected to take longer answers `{accepted: true, id}` at once. Progress comes as `view.progress {id, fraction}` events on the existing event channel, and the answer as `view.reply {id, display}`, as a run's reply comes. `view.cancel {id}` is idempotent, like `run.cancel`.
- **Identity** (4, 14): the derived handle is `hash(source handle, canonical view spec (14.1's typed canonical form), deriver id and version)`, the same rule 10.5 gives `blob.derive`. A derived value is a cache entry in the engine's store. It is reaped like any value; asking again recomputes it.
- **Section 10.5:** `blob.derive` stays for the byte-level derivatives it lists (thumb, tile, region, probe, peaks). `view.derive` is the analytic counterpart, structured rather than a spec string. 10.4's `want` gains `{kind: "view", view: {…}}` so a watched value's views can be made while the value is in memory (4.6, memo Never).
- **Hostile input** (18):
  - The view kind is from a closed list; unknown kinds are refused with `unknown_view`.
  - Every param is bounded: bins ≤ 65,536; profile points ≤ 1,048,576; regions ≤ 1,000,000 with the rest summarised as "other"; histogram2d bins ≤ 1,024²; `max.bytes` ≤ 64 MiB for the answer.
  - The `ref` must be a value the engine holds or can read under its `trusted_roots`, never a path it is told to open.
  - The answer's TABLE is checked by the editor like any engine value.
  - `view.derive` parsing joins `crates/proto/tests/hostile.rs`.

### 4.2 What the SDK base implements, once per language

- **Where it runs.** In the host layer, beside `acknowledgements` and `values.release` (Rust `crates/engine/src/host.rs:459`, Python `python/.../host.py:345`), not in nodes. So every engine on an SDK host answers it, including an engine that runs graphs with its own core (`Engine::with_core`, `EngineCore`, `crates/engine/src/lib.rs:361`): the host reads the value by DataRef from the store through `data::Plane`, which the core does not own.
- **The reductions:**
  - streaming histogram (1-D, per axis, 2-D joint);
  - statistics with percentiles (exact for up to 10^8 values; above that, a stated t-digest approximation);
  - profiles along an axis or a line (bilinear, defined);
  - N-D slices;
  - labelled-map regions (one pass, an integer hash or a dense table when the labels are compact);
  - select masks;
  - spectra (real FFT, Hann window by default);
  - scatter sampling (a fixed-seed reservoir, so Rust and Python sample the same rows).
- **Reading.** Every reduction reads the value through the SDK's existing array access: mapped `.npy` same-host, `blob.get` ranges remote. It is written to stream in chunks of at most 64 MiB, so memory is bounded whatever the size.
- **Rust:** a new crate, `plumcraft-views`: no GUI, no engine. It depends on `plumcraft-proto` and `plumcraft-store`, so it can be compiled into the editor too (4.5). `plumcraft-engine` calls it.
- **Python:** `plumcraft_engine/views.py`, numpy only.

### 4.3 Shared vectors: how the two implementations are held together

- **The file.** A new fixture file, `crates/proto/fixtures/views.json`, beside `curves.json` and `masks.json`, which already pin the Swift and Rust curve and mask maths. Each case holds:
  - an input array as `.npy` bytes in base64 (small, under 64 KiB), with dtype and shape;
  - a view spec;
  - the expected answer.
- **The rules, stated so both can meet them:**
  - **Counts** (histogram, regions' area, NaN and inf counts) are exact.
  - **Bin edges:** `lo` is inclusive; `hi` falls in the last bin; a value `x` goes to bin `floor((x − lo) / (hi − lo) × bins)` computed in f64, clamped to the last bin. That is today's rule in both SDKs, written down for the first time.
  - **Float statistics:** within 1e-9 relative of a reference computed in f64 with compensated summation. Neither SDK must reproduce the other's summation order; numpy's pairwise sum and Rust's Kahan both meet 1e-9 on the fixtures' sizes.
  - **Percentiles:** numpy's `linear` method, defined in the file.
  - **Profiles:** bilinear sample at stated points, within 1e-6.
  - **Sampling:** a stated seed and generator (a 64-bit SplitMix), so both sample the same indices.
- **The cases cover:**
  - every dtype the SDKs read;
  - NaN, ±inf and −0.0;
  - empty and singleton arrays;
  - rank 1 to 5;
  - a labelled map with negative labels and label 0;
  - the bounds (too many bins refused, too many regions summarised).
- **Who runs it:** the Rust SDK's tests, the Python SDK's pytest, and the engine conformance suite (each engine that lists `view.derive` answers the file's specs on the file's inputs).
- **Today's per-channel histograms in `stats`** join the file, which nothing checks today.

### 4.4 The editor: offering, asking, showing

- **`viewers::offered` by the matrix.** The offer is a function of media kind, dtype class (float, integer, bool) and rank:
  - integer rank 2 or 3 adds Regions and Treemap;
  - every numeric array adds Histogram, Statistics, Table and Profile;
  - rank ≥ 3 adds slice axes to Pictures and Values.

  `table: true` makes Table (or Plot) the **default**, first in the list, never the only chart. "One chart a value" (12.2 item 9) is withdrawn (decision 5).
- **A derived view is a viewer, and its data is a value.**
  - When a person picks Histogram, the pop-out's state gains `derive: {kind, params}`. Its value is the answer of `view.derive` on the port's current value, asked again when the port's value changes (a new handle) and cached by the derived handle.
  - The chart of a derived TABLE is the note 26 chart of a table, its spec kept in `meta.charts` against the port and the derive spec.
  - Brushing a histogram selects a value range of the source: the selection belongs to the value. Pictures of the same port then highlight those pixels through a `select-mask` view.
  - A derived value can be compared (Pin A: this run's histogram, B: the last run's), and opened in another window (`view.popout {another: true}`).
- **Commands:** `viewer.derive {popout, kind, params}` sets the view (one command, one state change, undoable when kept in `meta`). `viewer.histogram`, `viewer.slice` and `viewer.profile` are its named forms for the bars' controls. Every one is in the registry, so every shell, the CLI and the control port share it.
- **Every shell, nothing composed:** the core asks the engine, holds the answer, lays it out (charts by `plumcraft-chart`, tables by the table pages, pictures by the media service) and gives `popout.get` and `viewer.data` as today. A shell draws the same kinds of data it draws now. No shell computes a bin.
- **`plumcraft-chart` gains a treemap face.** The squarified layout moves from `chart_kinds.rs` (`treemap`, `:664`) into `plumcraft-chart`, behind the same `layout` entry the Rust SDK uses for face thumbnails. So a node whose output is a labelled map can show its treemap as a face, and the core and the SDK share one layout. The table-reading half (group-by over a TABLE's category columns) stays in the core, as note 26 12.3 kept it.

### 4.5 Who answers, and the editor's built-in views engine

1. **The producer.** The engine that made the value, if it lists `view.derive`. It has the bytes (sometimes still in memory) and knows the value's meaning (names, colour space).
2. **The editor's built-in views engine.** `plumcraft-views` compiled into the editor, run on the media service's workers, for an engine that doesn't list the capability. It reads the value the way the viewer reads it today: mapped from the shared store on the same host, or fetched with `blob.get` within the size the editor already accepts (512 MiB for tables). This makes the Rust implementation the universal fallback while keeping "implemented twice": the Python implementation serves Python engines where the bytes are.
3. **Disabled with a reason.** When neither can read the value (a remote engine without the capability holding a value too large to fetch), the view is offered disabled: "the engine that holds this value can't make views, and it's too large to fetch (2.1 GB)".

A value produced by one engine and held in a store another engine reads (a coordinator run across engines, note 15) is answered by its producer. The coordinator routes `view.derive` by the `ref`'s producing engine, which the coordinator knows from the fragment that made it.

### 4.6 Values not kept (memo policy "Never")

A node whose memo policy is "Never" (cache: never) keeps no value after the run. Its DataRef may be gone when the person asks for a view later. Two parts:

- **Watched:** a port whose pop-out has a derive spec sends the spec in its WatchRequest's `want` (4.1), so the SDK computes the view while the value is still held and publishes the small answer. The view is then as fresh as the run.
- **Not watched:** asking a view of a value that is gone answers `missing` (interface 4.6, "a missing blob is a state"). The view says "This value isn't kept; run again to see it" with a Run button. That is a rerun, the floor's F58 rule, not a graph edit.

## 5. The hard cases

- **Engines not on either SDK:**
  - **The craft MCP adapter** is built on `plumcraft_engine::Engine` (`crates/craft/src/lib.rs:37`, `:210`). It is on the Rust SDK host and gets `view.derive` for nothing once the host has it.
  - **An application's engine compiled into the core** uses `Engine::with_core` (public `crates/engine/src/lib.rs:594`). Its own core runs its graphs, but the host (ops, store, events) is the SDK's, so `view.derive` is answered by the host's reductions over its published values. It needs nothing unless it keeps values only in memory and never publishes them. Then `EngineCore` gains an optional `view()` hook, defaulting to "publish, then reduce". What this application's core does with its values was not read (private code); the hook covers both cases.
  - **An engine speaking the protocol by hand** (a third party, the DuckDB engine if it doesn't use the Python SDK) gets the editor's built-in views engine for any value the editor can read, and nothing else is required of it.
- **Very large arrays.** Reduce where the bytes are. Same host, the store file is mapped by whichever process reduces, the engine or the editor, so the cost is the pass over the data, not a copy. Remote, only the engine can do it without moving the bytes, which is why the producer answers first. Every reduction streams; the answer is small by construction (section 3.4's bounds). A 4 GB volume's histogram is one pass at memory bandwidth, a second or two, with progress events and cancel.
- **Remote HTTP engines.** `view.derive` is one more JSON-RPC method over HTTP (16.4). The derived value is in the engine's store and reaches the editor by `blob.get` like any value, small by construction.
- **What the editor does today, and whether it moves:**
  - **Picture decoding, SVG rasterising, table reading and sorting, waveform and spectrogram** (`plumcraft-media`) stay in the editor. They are about drawing values the editor already holds; tables must be paged where they are shown; and the waveform's per-column min and max is a drawing step.
  - **The chart aggregation over TABLE columns** (`chart_kinds.rs`: group-by, sum, stack) stays too: it works on tables, usually small, already read for the Table view.
  - **What moves into the SDKs** is the reductions of arrays that make a table out of something that isn't one (sections 3.2 and 3.4).
  - **Duplicated, and pinned:** the waveform's M4 reduction (`plumcraft_chart::layout`) is in the chart crate, which the Rust SDK shares; the Python SDK would get it through the wheel of note 26 12.3.
- **Cost in protocol surface:**
  - one capability, one op with two events and a cancel;
  - one `want` kind;
  - one fixture file;
  - interface sections 5.3, 6.1, 9.2 (two event kinds), 10.4, 10.5 and 18.

  That is smaller than `output.save` was.

### 5.1 The alternative: compute every view in the editor core

**Its case is real.**

- One implementation, in Rust, already beside the media service's workers and `plumcraft-chart`.
- It reaches every engine, on an SDK or not, with no capability and no protocol change.
- On the same host the store is mapped, so the editor reads a volume as cheaply as the engine does.
- Engine authors write nothing: "implemented once" beats "implemented twice".
- It is how note 26 built charts (decision 1A), and they work.

**Why the maintainer's inclination still wins where it matters:**

1. **Remote engines.** The bytes are where the engine is. A view of a 2 GB array held by an HTTP engine means moving 2 GB to look at 256 numbers. Interface 10.5 already chose engine-side derivatives for this reason.
2. **Values not kept.** Only the engine has the value at production time. A `want` spec in the watch lets it reduce then; the editor can't.
3. **Meaning.** The engine knows a value's channel names, colour space and what its labels mean (a background label, a "no region" value). An engine that wants a better region table than the base's can override the base method for its type. The editor can only guess.

**Where it does not win:**

1. **Engines not on an SDK.** The editor must do it anyway (4.5), so the Rust implementation runs in the editor too.
2. **Same-host tables.** These are cheaper in the editor, and stay there.

So the recommendation is the hybrid in 4.5: reductions in `plumcraft-views` (Rust), used by the Rust SDK and by the editor as the fallback, plus a Python port held to it by shared vectors. That is the maintainer's "twice", with the Rust one doing double duty.

## 6. Slice plan

Each slice is launchable, with a named person's check on the Mac scripted copy (and the iPad simulator where it draws something new).

| Slice | What | Gate | Person's check |
|---|---|---|---|
| 1. The offer, by the matrix | `offered` by kind, dtype and rank; flags pick the default only; "one chart a value" withdrawn; a table-flagged array gets Chart beside Plot | Core unit tests of the matrix; the registry test against egui's | A histogram node's (N, 2) output offers Plot, Table and Chart; a picture still offers Pictures first |
| 2. `plumcraft-views` and the vectors | The crate (histogram, stats, slice) with `views.json`; the Python port of the same three; both pass the file; today's `stats` histograms join it | Rust and pytest on the vectors; the hostile sweep | none (no UI) |
| 3. `view.derive` and Histogram for every numeric array | The op and capability in both SDK hosts; the editor asks the producer, else its built-in engine; Histogram offered for every numeric array of any rank; `viewer.histogram {bins, range, per, log}` | Engine conformance (`view.derive` on the vectors); shell conformance "a 3-D array's histogram is a view, with no node added" (graph hash unchanged) | A (D, H, W) volume popped out, View As › Histogram, Per › plane along 0 |
| 4. N-D slices and statistics | Pictures and Values with two axis pickers and sliders; Statistics with percentiles, per plane | Conformance of the slice and stats states; vectors | The same volume sliced along each axis |
| 5. Labelled maps: Regions and Treemap | `regions` and `select-mask` in both SDKs; the treemap layout moved into `plumcraft-chart`; Regions and Treemap offered for integer arrays; hover and click; linked highlight in Pictures | Vectors; conformance "a labelled map's treemap is a view, its click selects the region's pixels"; treemap areas within 1e-6 of their shares (existing test) | Connected Components' output popped out, Treemap, click a region, the picture shows it |
| 6. Profiles, scatter, spectrum, 2-D joint | The remaining reductions; density scatter as a picture mark | Vectors; fixtures of the display lists | A profile across a picture; the spectrum of a sound |
| 7. Not kept and remote | `want` view specs for watched ports; "run again to see it"; HTTP engines | Conformance with a `cache: never` node; an HTTP engine case | A never-cached node's histogram after its run |
| 8. Inputs | An input's View Value… (upstream or constant) | Conformance | An unwired input's constant shown as a table |
| 9. Python chart faces | The `plumcraft-chart` wheel (note 26 12.3's option) | Vectors of display lists run by pytest | A Python engine's 1-D output shows its line on the face |

## 7. Floor and conformance

- **The floor:**
  - F51 (view modes) is met.
  - F50, F54 and the note 04 rows "256-bin histograms" and "Tables plotted as points or segments" are met for what the research software had; FR6 goes beyond them.
  - The N-D dispatcher (2.3 item 2) is a protocol-2 promise, not a floor row; slice 4 meets it.
  - **Nothing in FR6 removes a floor capability.** The per-channel histograms in `stats` stay as the Pictures view's strip.
- **Engine conformance** gains `view.derive` against `views.json`, required of an engine that lists the capability.
- **Shell conformance** gains:
  - the matrix's offers;
  - a derived view on every shell, with the graph's hash unchanged after looking;
  - the treemap's click and linked highlight;
  - the "not kept" message.

## 8. Decisions for the maintainer

**Decided (the maintainer, 2026-11-25): "All A".** Option A of every point below stands as decided. With it, a principle, said when the controller hesitated over offering Treemap on any integer array because a picture that isn't a labelled map "may look odd": **"In general, it's the user's job to deal with odd, and your job to make seeing odd things easy."** So every view that can be computed for a value's kind, dtype class and rank is offered; none is withheld or gated because its result may be strange; flags and heuristics only choose the default.

Written for a checklist page: title, a line of context, options, the recommended option first (each A, decided).

1. **Where the reductions run.** The bytes can be in the engine (remote, not kept) or mapped beside the editor. Options:
   - **A (recommended):** in both SDK bases, with the Rust one also compiled into the editor as the fallback for engines without the capability.
   - B: SDK bases only (engines without it get no derived views).
   - C: the editor core only (no protocol change; remote and not-kept values fetched or lost).
2. **The protocol form.** Interface 10.5's `blob.derive` takes spec strings and was never built. Options:
   - **A (recommended):** a new structured op `view.derive` with its own capability, events and cancel; `blob.derive` kept for byte derivatives.
   - B: extend `blob.derive` with JSON specs.
   - C: only through the watch's `want` (no on-demand op).
3. **What a derived view answers.** Options:
   - **A (recommended):** a Display whose `full` is a new value with its own handle (`hash(source, spec, deriver)`), so it pops out, brushes, compares and caches like any value.
   - B: inline JSON only, no handle.
4. **Who answers.** Options:
   - **A (recommended):** the producing engine if it lists the capability, else the editor's built-in views engine, else disabled with the reason.
   - B: any engine with the capability.
   - C: always the editor when it can read the bytes.
5. **The offer rule.** Today flags gate Plot and Chart. Options:
   - **A (recommended):** offer by media kind, dtype class and rank from the matrix; a flag picks only the default; withdraw "one chart a value".
   - B: keep the flags, add views beside them.
6. **Where a derived view's choices are kept.** Options:
   - **A (recommended):** in the graph's `meta` like `meta.charts` (outside the result hash, undoable), so a saved graph reopens with its histograms.
   - B: in the session only, lost on close.
7. **Inputs.** Options:
   - **A (recommended):** every input can be viewed: a wired one shows the value it receives, an unwired one its own value.
   - B: outputs only, as today.
8. **Histogram defaults.** Options:
   - **A (recommended):** over all finite values, 256 bins over the finite range, NaN and ±inf counted apart; Per (plane along an axis), Bins, Range and Log as commands; 2-D joint on request.
   - B: today's per-channel default first, all-values second.
9. **What a labelled map is.** Options:
   - **A (recommended):** any integer array of rank 2 or 3 is offered Regions and Treemap; colour by a second port of the same size chosen in the view.
   - B: only arrays an engine marks as labels (a new `Form` field).
10. **The treemap layout's home.** Options:
    - **A (recommended):** move the squarified layout into `plumcraft-chart`, shared by the core and the SDK's face thumbnails; the group-by over TABLE columns stays in the core.
    - B: keep it in the core only (no treemap faces).
11. **How exactly Rust and Python must agree.** Options:
    - **A (recommended):** counts exact; bin edges by the stated rule; float statistics within 1e-9 relative of an f64 compensated reference; sampling by a stated seed.
    - B: bit-for-bit (forces one summation order on numpy: slower, more code).
12. **Values not kept.** Options:
    - **A (recommended):** a watched port's derived views are made at production time (`want`); otherwise "run again to see it".
    - B: every view of a not-kept value reruns the node.
13. **Python chart faces** (left undone after "Chart: a"). Options:
    - **A (recommended):** a PyO3 wheel of `plumcraft-chart`, so they can't drift.
    - B: a numpy port held by display-list vectors.
14. **Order.** Options:
    - **A (recommended):** slice 1 (the offer) first as a quick fix, then the infrastructure (slices 2 and 3) with the 3-D histogram as its first person's check, then labelled maps (slice 5).
    - B: labelled maps and the treemap first, on the editor fallback, the SDK op after.

## 9. What was not verified

- **Not launched:** nothing here was launched. Every "built" cell in section 2.2 is from the notes, STATUS and the code read at `63e9aad`, plus the launches recorded when each slice was built.
- **The application's own code was not read.** Whether its engine keeps values only in memory, and how its labelled maps are typed, was not checked, so 5's `EngineCore::view` hook is written to cover both.
- **Unmeasured:**
  - the cost of a streaming histogram over a 4 GB mapped volume (estimated at memory bandwidth);
  - numpy's pairwise sum against Kahan at the fixtures' sizes, against 1e-9.
- **Not read:** the stage-5 signals decision, which is quoted from the session's record, not from the application's notes.
