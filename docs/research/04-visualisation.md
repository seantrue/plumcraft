# 04: Visualising node data and data in flight

*2026-10-27. Research note for the generic base (working name FloCraft; see `../flocraft_effort_20261017.md`). Scope: how the generic UI shows node outputs, intermediates and live (in-flight) data for media beyond images, and the engine-side display payload that drives it. Sources read: `crates/ui-egui/src/viewer*.rs`, `curve_view.rs`, `mask_view.rs`, `textures.rs` (T16 inspector-window work in the `inspector-window` checkout); the research software's schema crate at the pinned rev `4009eac`; `python/src/research_engine/display.py`; `docs/engine-contract.md` › Display; `docs/protocol.md` › Plan, Status records, Display payload; `crates/engine/src/run.rs` (status reducer); `crates/local/src/engine.rs`; soundcraft (`crates/playback`, `crates/dsp/src/spectrum.rs`, `crates/ui-egui/src/waveform.rs`); the Cargo manifests of solidcraft, cadcraft, vectorcraft, figcraft and pdfcraft.*

## 1. Summary

- **One viewer host, many media viewers.** The current `Viewer` splits into a generic **host** (slots Current / A / B, pins, compare toggle, list item, sheet, own window, recipe, missing-file rerun, command surface) and a **registry of media viewers** keyed by a core **media kind** (`image`, `ndarray`, `table`, `scalar`, `text`, `audio`, `vector`, `mesh`, `tree`, `bytes`, `composite`, `sequence`). Each viewer declares a **capability set** (zoom, probe, channels, compare modes, sheet, play, stream, window, save, thumb).
- **Engines describe, the UI decides.** An engine maps each of its wire types to a media kind in `Schema`, and every display carries a **thumb**, zero or more engine-made **renditions** (derivatives), and a **full handle** (CAS md5 plus a local path or commonkit key, or a URL for a remote engine). The UI asks for further derivatives by name (`waveform`, `spectrogram`, `page`, `turntable`, `diff`) when it cannot or should not compute them itself.
- **The universal fallback is a raster.** Any engine, including a remote HTTP engine or an MCP-wrapped craft, can show anything by returning an `rgba8` rendition; the image viewer then gives zoom, sheet and pixel compare for free. Native viewers are an upgrade, never a requirement.
- **Image keeps everything it has today**, bit for bit: exact probe from the mapped full-resolution `.npy`, |A−B| from full arrays with max-of-block downscaling and NaN colouring, split, side by side, channel grid or tabs, log histograms, sheet, 1:1, own window that follows the selection. The image viewer is today's code moved behind the trait, with no extra copies.
- **Compare generalises to `compare(a, b) -> CompareResult { exact, summary, views }`**, with *exact* first-class (it is the research software's correctness rule). Probe generalises to a media-specific **locus** (pixel, sample, char offset, row/col, vertex, JSON path, byte offset, SVG element).
- **In flight:** today, a node's thumb appears as a `preview` status record when the node finishes, and `progress` carries only a fraction; the viewer itself only updates from the final reply. The proposal adds **live streams** per (node, port): `snapshot` (replace: progressive refinement), `append` (text tokens, audio chunks, table rows) and `log`. They ride the same status channel on every transport (in-process channel, AMQP `research.status`, HTTP SSE/WebSocket, stdio NDJSON), carry handles for bulk data, are throttled engine-side and coalesced UI-side, and are always reconciled by the final reply. The viewer **watches** what it shows, so only watched values get viewer-size partials.
- **Effort:** about 14 to 16 days for the core path (image moved behind the trait, table, scalar, text, tree, bytes, the live channel), 21 to 24 days with the feature-gated audio, SVG and mesh viewers (section 12); about 3 days overlap the FloCraft extraction.

## 2. What exists today and must not be lost

Every row is a feature the generic base must keep for the research software supporting www.virtimp.com (the maintainer's rule: never less functionality).

| Feature | Where | How it works today |
|---|---|---|
| Large view with zoom about pointer, pan, double-click fit, Fit and 1:1, nearest sampling past 1:1 | `viewer.rs`, `viewer_view.rs` | 8-bit RGBA preview (≤ `inspect_px`) drawn over a rect with the full array's aspect, so screen points map exactly to array pixels |
| View modes `auto color stretch clip heat labels angle` (+ `turn`) | `viewer.rs::MODES`, `display.py::views` | Part of `InspectRequest`; a change returns `ViewerAction::Rerun`; the list is hard-coded in Rust |
| Up to 4 channel pictures, grid or tabs, one shared zoom | `viewer.rs` `ChannelLayout` | `Display.previews` (`MAX_PREVIEWS = 4`) |
| Channel names, exact min/max, 256-bin histograms, log toggle | `channels_ui`, `viewer_plot.rs` | `Display.histograms` inline in the reply |
| Exact pixel probe on hover, click to pin, crosshair on every picture, readout for every compared value | `viewer_probe.rs` | Reads single elements from the mmap'd full-resolution `.npy` (`research_shm::NpyView`); nothing copied |
| Compare: Pin A / Pin B (B defaults to current), side by side, split with draggable divider, |A−B| mean or per channel | `viewer.rs`, `viewer_diff.rs` | Diff computed in the UI from the two full arrays, max-of-block so a single differing pixel survives, NaN counted and painted magenta, shapes must match |
| Lists: item stepper, contact **sheet** of up to 64 items with lineage labels | `sheet_ui`, `display.py::sheet_cells` | `InspectRequest.sheet`; `Display.sheet` cells with 256 px previews |
| Tables plotted as points or segments | `viewer_plot.rs` | `ArrayMeta.table` (N,k) arrays |
| Scalars and other values as text | `body_ui` | `Display.text`, `Display.value` |
| Recipe (lineage) panel, Save PNG with recipe, Save sheet | `recipe_ui`, `ViewerAction::Save*` | `Display.recipe`; `run.savePicture` |
| Own native window, follows selection, reruns, list item and mode even when the inspector is hidden (T16) | `window_ui` | The app calls `window_ui` each frame |
| Missing commonkit segment shows "re-run to refresh" and reruns once | `refresh_spent`, `saw_missing` | Reaped segments are a state, not an error |
| Canvas thumbnails that do not blink between runs | `engine/src/run.rs` reducer | `NodeReport.thumb`; `preview` status record carries the thumb the moment a node finishes; the reducer keeps the old thumb until a new one arrives |
| Progress per node | `StatusRecord.fraction` | At most every 100 ms per node; drawn on the node |
| Curves editor live preview through 8-bit LUTs with no run | `curve_view.rs` | `CurveData.picture` + `live` flag |
| Mask painter over an exact 8-bit preview | `mask_view.rs` | `MaskData.picture`; preview values round-trip exactly |
| Control channel: `ui.inspect`, `ui.screenshot`, `ui.set`, `ui.waitRun`; viewer exposes `image_rects`, `split_handle` for tests | `control.rs` | Used by kittest and the maintainer's sessions |

**Gaps the generic design must close** (they block non-image media and real in-flight viewing):

1. `Display` is an array record: `previews`, `histograms`, `meta: ArrayMeta`, `array: ArrayRef`. Anything else falls to `text` (Python truncates to 2000 chars) or `value`.
2. View modes are hard-coded in Rust (`MODES`) although only the engine knows them.
3. The viewer updates only from `RunReply.inspect`; status records carry `thumb` only, never the inspected display. A long node shows nothing in the viewer until the run ends.
4. `progress` carries a fraction but no partial result, so the research software's commands stage, a Perona–Malik iteration or a streaming text/audio generator cannot be watched.
5. Pins are requests, re-evaluated on every run, so "compare this run with the last one" is impossible: there is no pin-by-value.
6. Handles are paths or commonkit keys only; there is no CAS digest the UI can use to dedupe textures or fetch from a remote engine.

## 3. Media kinds and wire types

The core does not know an engine's wire types (`ARRAY`, `LAYER`, `EDITS`…). It knows a small, closed set of **media kinds**; engines map their types onto them in `Schema`:

```json
"wire_types": [
  {"id": "ARRAY",  "media": "ndarray", "views": ["auto","color","stretch","clip","heat","labels","angle"],
   "view_params": {"angle": [{"name":"turn","type":"FLOAT","default":1.0}]}},
  {"id": "LAYER",  "media": "composite", "fields": {"array": "ARRAY", "blend": "ENUM", "opacity": "FLOAT", "label": "STRING"}},
  {"id": "AUDIO",  "media": "audio"},
  {"id": "MESH",   "media": "mesh"},
  {"id": "SETTINGS","media": "tree"}
]
```

| Media kind | Holds | Bulk form on the single-system path (commonkit) | Native viewer |
|---|---|---|---|
| `image` | (H,W[,C]) for display, 8-bit or float | `.npy` segment | Image (today's) |
| `ndarray` | Any dtype/shape; `space`, `names`, `table` | `.npy` | Dispatch by shape: 2-D/3-D → Image; (N,k) table → Table/plot; 1-D → Plot; N-D → slice picker then Image |
| `table` | Columns with names and dtypes | `.npy` structured, or one `.npy` per column; Arrow IPC for a remote engine | Table grid + plot |
| `scalar` | Number, bool, enum, short string | inline JSON | Scalar (value, delta, history sparkline) |
| `text` | UTF-8 text, optional spans (tokens, highlights) | `.npy` of `u8` (bytes) so it mmaps like everything else, or inline when ≤ 4 KB | Text |
| `audio` | (frames, channels) f32 PCM + `rate` | `.npy` | Audio (waveform, spectrogram, playback) |
| `vector` | SVG document | `.npy` of `u8`, or a file path | SVG (usvg + tiny-skia) |
| `mesh` | vertices (N,3) f32, faces (M,3) u32, optional normals/colours; or GLB | several `.npy` segments in one handle, or a `.glb` file | Mesh (wgpu) |
| `tree` | JSON-like structure (settings bundles, curves, edits, MCP replies) | inline JSON up to a budget, else a `.json` handle | Tree |
| `bytes` | Opaque | file or `.npy` `u8` | Hex |
| `composite` | Named fields of other kinds (`LAYER`, `STYLE`) | per field | Composite: a field list, each field opens in its own viewer; the first viewable field is the thumb |
| `sequence` | Ordered items of one kind with a time base (video frames, pages, per-scale lists) | per item | Sheet + scrubber, item opens in its kind's viewer |

**Lists stay orthogonal**: any kind can arrive as a list (the research software's list mapping), and the host gives every kind the item stepper and the sheet.

## 4. Handles: how a viewer reaches the data

The display never carries bulk data, as today. It carries **handles** (the transport note defines the wire format; this is what the viewer needs from it):

```json
{"cas": "md5:9e10…", "media": "audio", "bytes": 3840128,
 "path": "/Users/…/commonkit/store/…npy", "key": "impc:v1:plumarr:9e10…:c",
 "url": "https://engine.example/cas/9e10…", "range": true,
 "meta": {"shape": [480000, 2], "dtype": "<f4", "rate": 48000}}
```

- **Resolution order in the UI:** `path` (mmap, single system) → `key` (commonkit, mapped to a path as `crates/shm` does now) → local CAS cache (`~/Library/Caches/<app>/cas/<md5>`) → `url` (download into the CAS cache; HTTP Range for slices when `range` is true) → ask the engine for a derivative.
- **The CAS digest is the texture and derivative cache key.** `TextureCache` keys by `cas` rather than path, so a cached node (same md5) never re-uploads, and a remote result already downloaded is never fetched twice. The md5 rule is commonkit's (dtype + shape + bytes) for arrays and alexandria's (plain bytes) for files; the handle says which (`"cas": "md5:"` vs `"cas": "md5npy:"`), so the UI can verify what it downloaded.
- **Full data is lazy.** Thumbs and the viewer-size rendition are always fetched; the full handle only when a feature needs it (probe, diff, playback, 1:1 zoom). A remote engine's viewer stays usable on thumbs and renditions alone.
- **Remote probe without download:** when the full handle is remote and large, probe and diff go to the engine as derivative requests (`probe` returns a few numbers; `diff` returns a rendition plus statistics). Results are exact either way: the engine reads the stored values.

## 5. Display payload v2

One record serves the reply's inspect entries, the node thumb, status records and the sheet. Field names are chosen so a v1 Python `Display` parses into it (section 11).

```json
{"media": "ndarray", "type": "ARRAY",
 "items": 7, "index": 2,
 "summary": "array Lab 812×808×3 float32",
 "full": <Handle>,
 "meta": {"shape": [812,808,3], "dtype": "<f4", "space": "Lab", "names": ["L","a","b"], "table": false},
 "view": {"id": "heat", "params": {}},
 "renditions": [
   {"kind": "rgba8", "role": "view", "caption": "L", "w": 1024, "h": 1019, "handle": <Handle>},
   {"kind": "rgba8", "role": "view", "caption": "a", "w": 1024, "h": 1019, "handle": <Handle>}],
 "stats": {"channels": [{"name": "L", "min": 0.0, "max": 100.0, "nan": 0,
                         "histogram": {"bins": 256, "lo": 0.0, "hi": 100.0, "counts": [256 ints]}}]},
 "value": null,
 "inline": null,
 "recipe": {…},
 "sheet": [{"label": "dim 150", "summary": "…", "thumb": <Rendition>}],
 "live": {"provisional": false, "seq": 0},
 "editor": {"curves": {…}}}
```

**Rendition kinds** (engine-made derivatives the UI can draw without understanding the data):

| Kind | Payload | Used by |
|---|---|---|
| `rgba8` | Handle to (h,w,4) u8, ≤ requested px | Image, every fallback, mesh turntable, PDF/page raster, spectrogram picture |
| `peaks` | Inline or handle: (levels, channels, bins, 2) min/max per bin at a power-of-two mip | Audio waveform, 1-D plots, thumbs |
| `spectrogram` | Handle to (frames, bins) f32 dB, with `hop`, `fft`, `rate` | Audio |
| `excerpt` | Inline text ≤ 4 KB, with `offset`, `total_chars`, `lines` | Text thumb, huge text paging |
| `svg` | Handle to SVG bytes | Vector, and engine-drawn plots |
| `mesh_lod` | Handle to a decimated mesh | Mesh thumbs, remote meshes |
| `plot` | Inline series `{x:[…], y:{name:[…]}, kind}` ≤ 4 KB | Scalars over a sweep, tables |
| `tree` | Inline JSON (depth ≤ 32, ≤ 64 KB) | Tree, composite fields |

**Inline budget.** Small derived data may travel in the message, as histograms already do: ≤ 4 KB for thumbs and status records, ≤ 64 KB for viewer replies. Above that it is a handle. This keeps "arrays never travel in messages" true for every bulk value.

**Derivative requests.** `InspectRequest` becomes a `WatchRequest` (section 8) that names what the viewer wants:

```json
{"node": "n3", "port": "audio", "index": 0, "sheet": false,
 "view": {"id": "auto", "params": {}},
 "want": [{"kind": "peaks", "px": 1600, "t0": 0.0, "t1": 12.5},
          {"kind": "spectrogram", "fft": 2048, "hop": 512},
          {"kind": "full"}],
 "px": 1024}
```

An engine that cannot make a derivative omits it; the viewer computes it from `full` if it can, or shows the summary. `want` defaults per media kind, so the research software's current request (`mode`, `turn`, `sheet`, `index`) maps onto `view` plus the default `want` of `ndarray` (one `rgba8` per channel view, histograms, `full`).

## 6. The viewer registry (Rust)

**Crate layout.** `flocraft-view` (L4, egui) holds the host, the trait and the core viewers (image, ndarray dispatch, table/plot, scalar, text, tree, hex, composite, sequence). Heavier viewers are separate crates behind cargo features so the base stays light: `flocraft-view-audio` (cpal, rustfft), `flocraft-view-svg` (usvg, tiny-skia), `flocraft-view-mesh` (wgpu via `egui_wgpu::Callback`). An engine pack's app crate can register its own viewers and editors at startup (the research software registers the curves and mask editors; the generic painter stays in core as FloCraft item 3 proposes).

```rust
bitflags! { pub struct Caps: u32 {
    const ZOOM = 1; const PAN = 2; const PROBE = 4; const CHANNELS = 8;
    const SIDE_BY_SIDE = 16; const SPLIT = 32; const OVERLAY = 64; const DIFF = 128;
    const SHEET = 256; const PLAY = 512; const STREAM = 1024; const WINDOW = 2048;
    const SAVE = 4096; const THUMB = 8192; const LINKED_VIEW = 16384;
}}

pub trait MediaViewer: Send {
    fn id(&self) -> &'static str;                       // "image", "audio", …
    fn accepts(&self, d: &Display) -> Option<u8>;        // score; highest wins, user can override
    fn caps(&self, d: &Display) -> Caps;
    fn want(&self, state: &ViewState, d: &Display) -> Vec<Want>;   // derivative requests
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ViewCx, slots: &Slots) -> Vec<ViewerAction>;
    fn thumb(&self, painter: &egui::Painter, rect: egui::Rect, d: &Display, cx: &ThumbCx);
    fn probe(&self, cx: &ViewCx, d: &Display, at: &Locus) -> Option<Readout>;
    fn compare(&mut self, cx: &mut ViewCx, a: &Display, b: &Display, how: CompareHow) -> CompareResult;
    fn live(&mut self, update: &LiveUpdate) -> LiveOutcome;  // apply a partial; Refetch on a gap
    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ViewCx) -> Vec<ViewerAction> { Vec::new() }
}
```

- **`ViewCx`** gives a viewer the shared services: the handle resolver (mmap/CAS/URL), the CAS-keyed texture cache, theme tokens, the audio output (one per app), the shared zoom/pan transform (so A, B and a split share one view, as today), the pinned locus, and the window flag.
- **The host keeps the generic behaviour** that today lives in `viewer.rs`: slots and pins, compare on/off and `CompareHow`, list stepper and sheet, Window/Back to panel (T16), recipe panel, missing-file rerun-once, `Rerun` dedup, and the commands. A viewer never talks to the engine; it returns `Want`s and actions, the host batches them into the next watch request.
- **Choosing a viewer:** highest `accepts` score; a "View as" menu lists every viewer that accepts the value, including Hex and the `rgba8` fallback, so the user can always fall back. The choice is per wire type and remembered in UI state (serde `state.rs`, readable over the control channel).
- **Never crash:** every viewer treats payloads as hostile, as the golden rules require: mesh indices bounds-checked against the vertex count, peaks shapes checked, text decoded lossily, JSON depth and size capped, `.npy` headers through `research-shm`'s checked parser. `panic_hunt` gets a fixture set of malformed displays per media kind.

## 7. The viewers

| Viewer | Shows | Probe locus and readout | Compare (`exact` is always reported) | Thumb | Computed where |
|---|---|---|---|---|---|
| **Image** (today's) | Channel pictures, modes from the engine's `views`, histograms, channels, 1:1, own window | (row, col): exact stored values of every channel, every compared slot | Side by side, split, |A−B| mean/per channel, max-of-block, NaN magenta; new: blink (toggle A/B on a key), and `exact` = every element equal including NaN positions | `rgba8` ≤ `node_px` | Diff and probe in the UI from mmap'd full arrays (unchanged); remote: engine `diff` derivative |
| **ndarray** dispatcher | 1-D → plot; (N,k) table → Table; N-D → axis pickers for the two image axes and sliders for the rest, then Image | Index tuple | Elementwise, with the same statistics as Image | First slice `rgba8` | UI slices the mmap (no copy) |
| **Table** | Virtualised grid (egui_extras `TableBuilder`), column stats, plus today's points/segments plot | (row, column) | Row/cell diff: added/removed/changed rows by key column or index, changed cells highlighted, numeric max abs diff | Mini plot or "N×k" badge | UI |
| **Scalar** | Big value with units, provenance (as today), history over recent runs, sweep plot when it is a list | n/a | Delta and relative delta; `exact` for bit-equal floats | Inline value badge on the node | UI |
| **Text** | Monospace/proportional toggle, wrap, line numbers, virtual scroll over an mmap'd byte buffer, search; spans (tokens, highlights) when the engine sends them | (line, col) and char offset; readout: char, code point, byte offset, token id | Line or word diff with the `similar` crate, side by side or unified; stats delta (chars, words, lines, tokens) | First lines `excerpt` | UI; token counts from the engine (it owns the tokenizer) |
| **Audio** | Waveform per channel (soundcraft's per-pixel min/max drawing), spectrogram, time ruler, loop region, playback with playhead | (time, channel): sample value, dBFS, frequency bin when on the spectrogram | Instant A/B switch during playback (sample-aligned, same playhead), overlaid waveforms, null test (A−B waveform and residual RMS dB), spectral difference | `peaks` sparkline | Peaks and spectrogram: engine derivative when offered, else UI from the mmap (rustfft, Hann window, as `soundcraft_dsp::spectrum`); playback: UI |
| **SVG** | Rasterised at the current zoom (crisp at every scale), element outline on hover | Element id/path under the pointer, its attributes | Raster |A−B| through the Image comparer at a common resolution, plus a structural diff of the element tree | `rgba8` raster | UI (usvg 0.48 + tiny-skia 0.12, the versions vectorcraft and figcraft use) |
| **Mesh** | Orbit/pan/zoom camera, flat, smooth and wireframe shading, bounding box, normals toggle | Picked vertex or face: index, position, normal | Linked cameras side by side; overlay; per-vertex distance colouring when topology matches, Hausdorff otherwise (engine derivative) | Engine `rgba8` turntable frame or UI offscreen render cached by CAS | UI wgpu (wgpu 30 as solidcraft); remote: `mesh_lod` |
| **Tree** | Collapsible JSON tree, values typed, copy path | JSON path | Structural diff: added, removed, changed keys | Key count or first keys | UI |
| **Hex** | Offset/hex/ASCII, virtualised over the mmap, magic sniff, entropy strip | Byte offset | First differing offset, differing ranges | Size and sniffed type | UI |
| **Composite** | Field list; each field opens in its viewer below (LAYER: picture plus blend/opacity/label) | Delegated to the field | Per field | First viewable field | UI |
| **Sequence** | Scrubber and sheet; an item opens in its kind's viewer (video frames, PDF pages, scales) | (item, inner locus) | Per item, plus "first differing item" | Sheet strip | UI; frames as `rgba8` |

**Audio specifics.** Use cpal 0.18 (soundcraft's version) with soundcraft's pattern: the audio callback owns a lock-free reader over an `Arc` of the mapped PCM, commands go in through a channel, position comes back through an atomic, and with no device the transport runs on a silent clock. One output per app, shared by all audio viewers; starting playback in one slot stops the other unless A/B is engaged. Different sample rates between A and B are resampled by an engine derivative (`{"kind": "full", "rate": 48000}`), never silently in the UI. symphonia is not needed in the viewer: engines hand over PCM.

**Mesh specifics.** egui 0.36 on wgpu renders through `egui_wgpu::Callback` into the viewer's rect; the UI keeps one pipeline and uploads vertex buffers keyed by CAS. A (N,3) f32 + (M,3) u32 pair of `.npy` segments keeps the commonkit rule; GLB (solidcraft writes it with the `gltf` crate) is accepted as a `bytes` handle with `"format": "glb"`.

## 8. Live and in-flight data

### 8.1 What exists

`queued`, `started`, `progress` (fraction), `preview` (thumb when a node finishes), `done`, `cached`, `error`, `cancelled`, ordered by `(ts, seq)`, generation-filtered, never moving backwards, reconciled by the final reply (`crates/engine/src/run.rs`). The local engine emits the same records (`crates/local/src/engine.rs`). This all stays.

### 8.2 What is added

**Watch list.** The run request's `inspect` becomes `watch: [WatchRequest]` (section 5). Watched values get viewer-size displays in status records as soon as they exist, not only in the reply. Unwatched nodes get thumbs only. The UI can change the watch list mid-run with a `watch` message (same correlation id) so clicking a running node starts showing it at once; an engine that cannot change it mid-run applies it at the next run.

**Status record v2** (a superset; old fields keep their meaning):

```json
{"v": 2, "run_id": "…", "generation": 17, "node_id": "n7", "seq": 41, "ts": 1792170000.123,
 "state": "progress", "fraction": 0.42, "stage": "dabs 3/7", "worker": "Step.gpu@gpu-host:4182",
 "thumb": <Rendition>,
 "live": [{"port": "result", "stream": "snapshot", "seq": 3, "provisional": true,
           "display": <Display>},
          {"port": "text", "stream": "append", "seq": 118, "offset": 5120,
           "inline": "the next tokens…", "handle": null},
          {"port": "audio", "stream": "append", "seq": 22, "offset": 960000, "frames": 48000,
           "handle": <Handle of this chunk>},
          {"port": null, "stream": "log", "level": "info", "text": "pyramid level 2"}]}
```

| Stream | Meaning | UI behaviour |
|---|---|---|
| `snapshot` | A whole provisional value (progressive refinement: each the research software's commands stage, a Perona–Malik iteration, a diffusion step, a coarse-to-fine render) | Replace; keep only the newest per (node, port); drawn with a provisional marker (hatched border on the thumb, "running: stage 3/7" in the viewer) |
| `append` | A growing value (text tokens, audio chunks, table rows, frames) with a contiguous `offset` | Append in order; a gap or out-of-order `seq` makes the viewer fetch the full handle once (`LiveOutcome::Refetch`); text and audio play/scroll as they arrive ("follow" toggle) |
| `log` | Node-scoped messages | Shown in the node's status tooltip and a run log; never in the value |

**Rates and budgets.** The engine throttles to at most one record per node per 100 ms (today's rule), except `append` chunks, which it batches to ≥ 50 ms or ≥ 64 KB. Thumbs in status records stay ≤ `node_px`; snapshots for watched values ≤ `inspect_px`. The UI drains the channel each frame (as now), coalesces snapshots per (node, port), and repaints only when something visible changed.

**Reconciliation.** The final reply (or a `done` record) replaces provisional values; `provisional: true` never survives `done`. Old generations are dropped as today, and a cancelled run keeps the last good non-provisional display.

**Transports.** The record is the same JSON on every engine kind; only the carrier differs: an mpsc channel for the compiled-in engine, AMQP topic `research.status` / `run.<correlation>` for pipekit (records stay pixel-free: handles only), NDJSON on stdout/stderr-separated pipes or a socket for a separate-process Rust engine, Server-Sent Events (or a WebSocket for the bidirectional `watch` update) for HTTP. Polling is the floor: `GET /runs/<id>/status?after=<seq>` returns the same records, so an engine that cannot push still supports progress, live previews and state tracking.

### 8.3 On the canvas

- **Node thumbnails** use the viewer's `thumb()` (section 7), so an audio node shows its waveform, a text node its first lines, a scalar node its value. Thumbs update from `preview` and `snapshot` records, keep the last good one while a new run computes (today's no-blink rule), and grey out when their handle is reaped.
- **Progress** stays a bar on the node; `stage` text appears under it.
- **Wires in flight.** A wire whose source is running or has a fresh value shows a small badge with the item count and size (from the handle), and an `append` stream animates the wire. Hovering a wire shows the source's thumb; clicking it inspects the source (today's behaviour).

## 9. Compare and probe, generalised

- **`CompareHow`**: `SideBySide`, `Split` (image, SVG, spectrogram), `Overlay` (waveforms, meshes, plots), `Blink`, `Difference`. The host offers only what both viewers' `caps` allow; the toolbar is built from caps, not hard-coded.
- **`CompareResult { exact: bool, summary: String, stats: Vec<(String, f64)>, views: Vec<Rendition or local texture> }`**. `exact` uses the bit-for-bit rule (equal dtype, shape and bytes; same NaN positions), computed from the CAS digest first: equal md5 means exact with no data read. This turns exactness checking into a viewer feature for every type.
- **Across nodes, runs and engines.** Pin A and Pin B keep their current meaning (a request, re-evaluated each run). A new **Freeze** pins by value: the slot keeps its handle (CAS), so "this run vs the last one", "before vs after this edit" and "Python engine vs Rust engine" become possible. Frozen handles are protected from reaping only while pinned (the transport note's lease).
- **Different kinds** compare only through a common rendition (both `rgba8`), and the UI says so.
- **Probe** uses a `Locus` enum (`Pixel{row,col}`, `Sample{frame,ch}`, `Text{line,col,offset}`, `Cell{row,col}`, `Vertex(u32)`, `Face(u32)`, `Path(String)`, `Offset(u64)`, `Element(String)`, `Item(usize, Box<Locus>)`). Hover reads, a click pins, and the pinned locus is drawn on every slot and read out for every compared value, as today.

## 10. Commands and control

Everything is a command, so the viewer's actions get ids the UI, CLI, control channel and MCP share: `viewer.open {node, port, index}`, `viewer.viewAs {viewer}`, `viewer.view {id, params}`, `viewer.pin {slot, freeze}`, `viewer.compare {on, how}`, `viewer.probe {locus}`, `viewer.sheet {on}`, `viewer.window {on}`, `viewer.play`, `viewer.stop`, `viewer.seek {t}`, `viewer.follow {on}`, `viewer.save`. `ui.inspect` stays as an alias of `viewer.open`. Viewer state (chosen viewer per type, view, slots, locus, playhead) lives in the serde UI state so the control channel can read it, and `image_rects` / `split_handle` generalise to `viewer.geometry` for tests.

## 11. Migration: the research software on the new base

- **Parse v1 as v2.** The schema crate accepts today's `Display`: `previews` → `renditions` (kind `rgba8`, role `view`); `histograms` → `stats.channels[].histogram`; `meta` → `meta`; `array` → `full`; `text` → `summary`; `value`, `sheet`, `recipe` unchanged; media is `ndarray`, or `table` when `meta.table`. `NodeReport.thumb` and `StatusRecord.thumb` → `Rendition`. `curve` and `mask` move under `editor` (FloCraft item 3). The Python engine therefore runs unchanged on day one, and the fixture test (`python_plan_reply.json`) pins the mapping.
- **Modes.** `MODES` disappears from Rust; the engine lists them in `wire_types[ARRAY].views` (FloCraft item 5). `InspectRequest.mode/turn` ↔ `view.id/params.turn`.
- **Efficiency.** The image viewer is the existing code (`viewer.rs` body, `viewer_diff.rs`, `viewer_probe.rs`, `viewer_view.rs`, `viewer_plot.rs`) behind the trait: same mmap, same textures, same zero-copy probe. The only new per-frame cost is a viewer lookup by id. CAS-keyed textures remove re-uploads that path keys cause today when a cached node is republished under a new path.
- **Python engine changes (small):** `display.py` emits `media`, `full.cas` (it already computes the md5 for the key), and honours `watch` by publishing the watched display in its `preview` record; the research software's commands stages report `snapshot` partials through the existing progress callback (opt-in, watched nodes only).
- **Acceptance.** A new `docs/acceptance/` session repeats T16 and the M3 inspection checklist on the new base before any other media lands, so the maintainer confirms nothing was lost.

## 12. Effort

| Step | Days |
|---|---|
| Display v2 types, v1 compatibility, fixtures, `Rendition`/`Handle`/`Locus` | 1.5 |
| Host/registry split; image viewer moved behind the trait; caps-driven toolbar; T16 re-test | 3 to 4 |
| CAS-keyed texture cache and handle resolver (path, key, CAS cache, URL with Range) | 1.5 |
| Live channel: watch list, status v2, snapshot/append/log, reducer and canvas changes, provisional marking | 3 to 4 |
| ndarray dispatch, Table, Scalar, Tree, Hex, Composite, Sequence | 2.5 |
| Text viewer with `similar` diff and streaming | 1 |
| Freeze pins, generic `CompareResult` with md5 fast path | 1 |
| Audio viewer (cpal playback, peaks, spectrogram, A/B, null test) | 2 to 3 (feature crate) |
| SVG viewer | 1 (feature crate) |
| Mesh viewer (wgpu callback, picking, overlay) | 3 to 4 (feature crate) |
| `panic_hunt` fixtures per kind, kittest snapshots | 1 |

Core path (everything but audio, SVG and mesh): about 14 to 16 days. With all three feature viewers: about 21 to 24 days. These are additive to the FloCraft extraction estimate where they overlap its items 3 to 5 (about 3 days of overlap).

## 13. Open questions

1. **Text bulk form.** `u8` `.npy` keeps one mmap path for all bulk data; plain files are friendlier for remote engines. Proposal: both are handles; the resolver does not care.
2. **Who computes spectrograms and peaks?** Engine derivatives are cheaper over a network; the UI is cheaper locally. Proposal: the viewer asks, the engine may decline, the UI computes from `full` when local.
3. **Mid-run watch changes on pipekit.** Status flows one way on AMQP; a `watch` update needs the control fanout (as cancel does). Check with the pipekit transport note.
4. **Mesh thumbs.** Offscreen wgpu render per node vs engine raster. Proposal: engine raster when offered, else one shared offscreen target, rendered lazily and cached by CAS.
5. **Inline budget numbers** (4 KB thumbs and status, 64 KB replies) need a measurement on AMQP and SSE.
6. **Tokens.** Token counts need the engine's tokenizer; the text viewer shows chars/words/lines without it.
