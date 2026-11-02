# Roadmap

Ideas and asks not yet in a milestone. `docs/plan.md` holds the milestones; `docs/STATUS.md` says what's next.

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

