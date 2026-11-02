# 16. Viewers: a preview and a window for every type, and values on wires

The maintainer, 2026-10-30:

- "We need a preview and full popout window for every base type, and an architecture to make it easy to add new ones .. check jetbrains for an array display."
- "Popout previews should not block interaction in the editor, and updates should update the popout display."
- "Add preview for data on a wire."

## 1. Today

- **The Watched panel shows one value** (`crates/ui/src/watched.rs`).
  - Pictures, SVG and craft documents go through a `MediaViewer` trait (`media.rs`), which its own docs call "the shape a later registry can adopt".
  - Tables, arrays, text, scalars and audio are hard-coded branches of the panel.
- **No viewer at all:** a PDF, a mesh, a JSON tree, a list of items (other than stepping through it) and raw bytes get Open / Open with….
- **No windows:** there is no way to see two values at once, or a value bigger than the side panel.
- **No values on wires:** hovering a wire shows nothing about the value on it. A click on a wire watches it (the Watched panel).

## 2. The architecture

### 2.1 One trait, one registry

```rust
pub trait Viewer {
    /// Take the value: its display (summary, renditions, stats), its full bytes when read, and
    /// the rendition's texture. Cheap when nothing changed; heavy work goes to a worker thread.
    fn sync(&mut self, ctx: &egui::Context, v: &ViewInput<'_>);
    /// Small, no controls: a node's hover, a wire's hover, the top of the Watched panel.
    fn preview(&mut self, ui: &mut egui::Ui, max: egui::Vec2, tokens: &Tokens);
    /// Everything, with its controls: the Watched panel and a pop-out window.
    fn full(&mut self, ui: &mut egui::Ui, tokens: &Tokens);
    /// Its state, for `ui.inspect`.
    fn inspect(&self) -> Value;
}

pub struct ViewerKind {
    pub id: &'static str,                  // "array.grid"
    pub label: &'static str,               // "Values"
    pub accepts: fn(&Display) -> bool,     // by media kind, media type (application/pdf), shape
    pub make: fn() -> Box<dyn Viewer>,     // a fresh viewer: each window has its own state
}
```

- **The registry** is an ordered list of `ViewerKind`s. For a value it offers every kind that accepts it, the first being the default. A value with several (an array: Pictures and Values) shows them as tabs.
- **Fallback:** the last entry accepts everything, as raw bytes in hex.
- **Adding a type** means writing one `Viewer` and adding one line to `viewers::registry()`. Nothing else in the editor changes.
- **Shared plumbing:** zoom and pan (`ZoomPan`), worker jobs (`Job`), and reading the full value (`Watched::update`) come from the module, not each viewer.

### 2.2 The viewers, per base type

| Media | Viewers (default first) | New? |
|---|---|---|
| `image` | Picture: zoom, pan, fit | ported |
| `vector` | SVG: re-rasterised at the zoom shown | ported |
| `doc` | The rendition, Open in its app; **PDF pages** (hayro, pure Rust): page stepper, zoom | PDF new |
| `ndarray` | Pictures (the seven views, channel stats and histograms); **Values: a numeric grid like PyCharm's data viewer** (format such as `%.3f`, colour by value, a slice picker for 3-D and more, header rows and columns, scrolling over millions of cells) | Values new |
| `table` | Grid: sortable columns, a bar for numeric columns, colour by value | extended |
| `scalar` | The value, large, with its type | ported |
| `text` | Text: line numbers, wrap, find | extended |
| `audio` | Waveform, spectrogram, player | ported |
| `tree` (JSON) | **Tree: collapsible, with paths and counts**; raw JSON | new |
| `sequence` (lists) | **Items: a grid of each item's preview**; a click shows one | new |
| `composite` (tuples) | **Parts side by side**, each by its own viewer | new |
| `mesh` | Open in its app (no 3-D viewer yet) | — |
| `bytes` and anything unknown | **Hex: offset, bytes, ASCII** | new |

### 2.3 Pop-out windows

- **What a pop-out is:** a viewer in a window of its own (an egui viewport: a native window on macOS, an in-editor window where the backend has only one window, as in kittests). Every viewer's `full` is what it draws.
- **Not modal:** pop-outs never block the editor. Each draws in its own viewport, and the editor keeps working while several are open.
- **Live:** a pop-out follows a port, not a value. Opening one adds its `{node, port}` to the run's watch list, so every run (and every `preview` record while it runs) sends its display. The pop-out syncs when a newer display arrives. Closing it removes the watch.
- **Commands** (everything is a command):
  - `view.popout {node, port, viewer?}`: opens a pop-out, or brings an open one forward;
  - `view.close {id}`;
  - `view.list`.
  - `ui.inspect` reports every pop-out and its viewer's state.
- **Where they open from:** the Watched panel (Pop out), the node menu's outputs, and a wire's menu.

### 2.4 Values on wires

- **The cache:** the editor keeps the newest display of every port it has seen. Sources are the run replies' watch entries, the `preview` records, pop-outs, and the nodes' thumbnails.
- **Hovering a wire** shows a tooltip with:
  - the type and summary, and the value's `preview`;
  - when the port's value isn't known yet, the source node's state and thumbnail;
  - after a short wait, a **peek**. A peek is a watch for that one run, so the engine sends the value; memo hits make it cheap. It doesn't change the Watched panel.
- **On a wire:** a click watches it (as today), a double-click pops it out, and the wire's menu has Pop out.
- **Tests:** a kittest covers hover, peek and pop-out.

## 3. Order of work

| Step | What |
|---|---|
| 1 | **Done.** The `Viewer` trait and registry; port picture, SVG, document, array pictures, table, text, scalar and audio into it; the Watched panel draws through it (no change visible) |
| 2 | **Done.** Pop-out windows: viewports, the watch list, `view.*` commands, `ui.inspect`; kittest and a live check that the editor stays interactive and pop-outs update |
| 3 | **Done but tuples.** New viewers: array Values grid (PyCharm-style), JSON tree, items grid, tuple parts, hex, PDF pages |
| 4 | **Done.** Values on wires: the port cache, hover previews, peek, double-click pop-out |

## 3a. As built (2026-10-30)

All four steps are done.

- **Code:**
  - the viewers are in `crates/ui/src/viewers/`: `mod.rs` (trait, registry, `Pane`), `basic.rs`, `grid.rs`, `pdf.rs`, `wrap.rs`;
  - pop-outs are in `crates/ui/src/popouts.rs`, and the values on wires in `crates/ui/src/wire_view.rs`;
  - the commands are in `crates/session/src/view_cmds.rs`, and watch roles in `RunState` (`WatchRole`: `Panel`, `Popout(id)`, `Peek`).
- **Different from the design:**
  - **The Watched panel keeps its own viewers.** Its picture, SVG, document, array and audio viewers are still `EditorApp::media` and `EditorApp::audio`, which the control channel and several tests drive directly. The other kinds are drawn by a `Pane` as tabs beside them. Pop-outs use only the registry. Moving the panel onto the registry is left to do.
  - **Tuples have no viewer yet.** The "parts side by side" viewer (composite) isn't built, and meshes still open in their app.
  - **Tree takes tables too:** the JSON tree also accepts a table and any object or list value.
  - **Rows are tables:** a list of lists (Table to Rows, GridCraft's Read range) opens in the table viewer by default. An all-text first row names the columns; otherwise they are c1, c2, …. Value and Tree are still offered (the maintainer: "Rows should be visualized with the table viewer").
  - **No hex view for inline values:** short values arrive inline, with no bytes, so they don't offer Bytes.
- **Fixed on the way:**
  - `run.watch` replaced the whole watch list; it now replaces only the panel's entry.
  - A reply's displays are taken by position, so two entries on one port each get their own.
  - Opening a graph closes its pop-outs.
  - Offscreen pictures run no animations, so in-editor windows are fully drawn.
  - The editor's fonts have no right arrow or pop-out glyph: ASCII "->" instead.
- **Checks:**
  - **Kittests** (`crates/ui/tests/kittest_viewers.rs`):
    - pop-outs follow their port, aren't modal, and update after an edit;
    - the panel's tabs;
    - the array Values grid's range;
    - wire hover, both a known value and a peeked one.
  - **Unit tests:**
    - the session's pop-out commands;
    - format and layout for the grid;
    - hex lines and colour by value;
    - a hand-made PDF rendered to pixels.
  - **Live:** native pop-outs in the release editor on the analytics demo (`claude-debug/multi/popouts_probe.py`). The PDF, the histogram rows as a tree and the noise as a Values grid were all open while an edit and a run updated them.

## 3b. egui_extras for tables and SVG (2026-10-30)

The maintainer: "Look at egui_extras for table and svg views".

| Piece | What egui_extras 0.36 does | Decision |
|---|---|---|
| **Tables** (`TableBuilder`) | A header that stays put while the rows scroll; columns resizable by drag, with initial, auto, exact or remainder sizing; striped rows; only the rows on screen laid out (`body.rows`); row selection and `scroll_to_row` | **Adopted** for `table.grid`, now used by the Watched panel too (it had its own table before). It fixes a flaw in the hand-made table: its header sat outside the scroll area, so on a wide table it didn't scroll sideways with the values. Columns start as wide as their content needs, because an auto-sized column left right-aligned numbers nowhere to go. Sorting by a header click stays |
| **Array Values** (`array.grid`) | `TableBuilder` skips only off-screen rows, never columns: a 384-wide array would lay out about 11,000 cells a frame, and a 4096-wide one far more | **Kept our own** grid, which reads and draws only the cells on screen, both ways |
| **SVG** (the `svg` image loader) | Rasterises with resvg when an image is first asked for at a size, **synchronously on the UI thread**, with no limit on the size asked for, and caches a picture per size | **Kept for icons**, as it already is (`crates/ui/src/icons.rs`). **Not for values:** a large SVG from an engine (VectorCraft's traces run to hundreds of kilobytes) would stall the editor. A zoomed view would ask for an unbounded raster, and a bad file has no fallback. Our `SvgView` parses and rasterises on a worker thread within limits, re-rasterises at the zoom shown, and falls back to the engine's picture |
| **Code views** (`syntax_highlighting`) | Simple highlighting for a few languages without syntect; with syntect, many more (a large dependency) | Not adopted; worth a look for the Raw JSON view and text values |

## 4. Also done

- **Rows to Table** (`seq.rows_table`), the inverse of Table to Rows, in both SDKs, with a round-trip test (the maintainer: "Transforms like table to rows should always have a symettrical node to go the other way").
- **A rule for the future:** every transform node ships with its inverse.
