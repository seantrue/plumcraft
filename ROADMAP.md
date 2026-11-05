# Roadmap

Ideas and asks not yet in a milestone. `docs/plan.md` holds the milestones; `docs/STATUS.md` says what's next.

## Next: BLAKE3 handles for large local arrays (D3's measurement)

*The maintainer, 2026-11-04: add it to the roadmap as the next thing.*

Decision D3 (`docs/plan.md`) made MD5 the default handle digest and said: "Measure hashing on the development machine before considering BLAKE3 for large local arrays." That measurement now exists, and it favours BLAKE3.

- **What a crossing costs.** On an M4 Max with 0.2.1, a 12 MP float32 RGB array (144 MB) crossing from one engine to another, through the coordinator, adds about 550 to 600 ms; at 3 MP (36 MB), 130 to 190 ms. That is more than the nodes on either side take. Hosting doesn't change it: in-process and process engines were within noise of each other, since a crossing goes through the store either way (`crossing_inputs`, `crates/coordinator/src/runner.rs`).
- **Hashing is the largest known part.** MD5 of 144 MB took 190 ms (about 0.76 GB/s, as note 03 estimated); BLAKE3 took 10 ms (`b3sum`, multithreaded). The rest of the crossing hasn't been profiled: whether the read hashes again, or the write itself costs.
- **The same width.** BLAKE3 can produce output of any length. Its 16-byte output is the first 16 bytes of its standard 32-byte one, so a 128-bit digest is 32 hex digits, MD5's size, and commonkit's digest slot and the store paths don't change. That gives about 2⁶⁴ collision resistance and 2¹²⁸ against a preimage. MD5 nominally promises the same 2⁶⁴, but an attacker can craft MD5 collisions cheaply. Stores that several clients write keep full 256-bit `b3` handles (64 hex), as D3 says.
- **Keys must not mix algorithms.** An MD5 key and a truncated BLAKE3 key look alike in a 32-hex slot, so BLAKE3 needs its own commonkit domain or version (pycommonkit's `docs/rust-crate-plan.md`). Handle text already says which: `b3:nd1:…` against `md5:nd1:…`. The `nd1` recipe (dtype, shape, C-order bytes) is unchanged.
- **To do:**
  - Profile `Store::publish_array` and its read path (`crates/store`), to confirm hashing is the cost.
  - Add commonkit's BLAKE3 key helper, which D3 held back "only if D3's measurement favours BLAKE3".
  - Let the store and both SDKs publish large arrays under `b3`, using the data plane's algorithm negotiation (`data_plane.algos`, note 03).
  - Keep MD5 where interop needs it: alexandria items and the research checkpoints.
  - Re-measure.
- **Also worth weighing** (note 03, storage tiers): no hashing until a handle is needed, with a derivation-addressed `drv` handle standing in. An in-memory fast path between two in-process engines would skip the store for a crossing, but helps only in-process engines.

## Externalize colours and theme materials

**Done for the editor (E1.3, 2026-10-27):** `plumcraft-ui` takes every colour from craft-ui tokens (`crates/ui/src/theme.rs`), node states included; family colours come from the catalogue's wire types or a stable hash of the family id.

**Accepted for `plumcraft-view` (the maintainer, 2026-11-01):** the first minimal window keeps its fixed colours, public release included.

## Specialist viewer windows per data type

*Asked by the maintainer, 2026-10-27: "consider using the expanded window display (click on the preview) as a way to generate interactive specialist windows for different data types. Some can be built in to PlumCraft, some could be added by a derived application. External plugins would be a stretch."*

Clicking a node's preview would open an expanded window for its value, as the research software opens its picture in its own window (T16). That window would be **the viewer for the value's media type**, interactive, not just bigger: for example, zoom, pan and probe for an image; a scrubbable waveform and spectrogram with selection and loop for audio; a sortable, filterable grid for a table; search and paging for text; a slice picker for an N-d array.

- **Registry keyed by media type.** The editor picks the window from the display's `media` (spec 10.1) and the wire type's views (`WireType.views`, `default_view`), falling back to the generic Watched view. One trait, roughly: `title`, `show(ui, display, resolve)`, and optional `thumb`, `probe` and `compare`. That is the viewer-host design in `docs/plan.md` section 7 and research note 04, given a window.
- **Three tiers:**
  1. **Built in to PlumCraft:** text, table, ndarray/plot, audio, and image once an image engine exists.
  2. **Added by a derived application:** the research software's Lab/curves/mask editors, for example. The app registers more viewers through the same trait at startup (compiled in), without forking the base.
  3. **External plugins (a stretch):** viewers loaded at run time. This means a stable ABI or a separate process drawing into a shared texture, which is a lot of surface; only if a real need appears.
- **Windows are interactive, so they act through commands.** Anything that changes the graph (for example setting a widget from a probe, or saving a selection) goes through the registry, so the control channel and tests reach it too.
- **When:** after the editor's E2 and E3 (files, undo, groups), alongside the image viewer. The streaming plan (research note 09) wants live versions of the same views (a scrolling spectrogram, a scope, a live text view), so the trait should allow a view to be fed incrementally.

## Mask painting through PhotoCraft

The maintainer, 2026-10-31: "mask painter could be done in PhotoCraft, with some automation". Nodes such as OpenCV's Inpaint take a mask as an array, and PlumCraft has no mask painter (the floor's `editor.mask`). Rather than build brushes into the editor, open the picture in PhotoCraft from a node (the craft pack and "Open in …" are there), paint in the app, and bring the mask back as an ARRAY with automation over its MCP server. To work out: how a painted layer gets back to the graph, and re-running when it changes.

