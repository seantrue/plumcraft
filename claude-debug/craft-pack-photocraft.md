# PhotoCraft pack (Worker A, 2026-10-28)

## Status

**Done.** `crates/craft/packs/photocraft.toml` (26 curated nodes plus the `pc.run` escape hatch), `crates/craft/packs/photocraft-demo.json`, `crates/craft/tests/pack_photocraft.rs` (3 tests, all pass against `photocraft-cli` 0.15.1). The smoke test (`plumcraft-craft-engine --craft photocraft --smoke claude-debug/craft-mcp/corgi.jpg`) passes every node with its defaults on the corgi.

## How PhotoCraft is driven

| Manifest | PhotoCraft |
| --- | --- |
| `command` | `photocraft-cli mcp` (headless engine), with `--automation-read-root` / `--automation-write-root` |
| `tools.list` | `command_list` (bare array of `{id, label, menu, params (prose), …}`, 940 commands) |
| `tools.run` | `command_run {id, params}` |
| `actions.open` | tool `doc_open {path}` (makes the document active) |
| `actions.save` | tool `doc_save {path}` (`.pcraft`: a stored zip; deterministic for the same file name) |
| `actions.export` | tool `doc_export {path, format}`; `format` is an extension (png, jpg, pdf, psd, tif, webp, exr, …) |
| `actions.clear_history` | command `history.clear` |
| `actions.close_all` | command `file.closeAll` (round 2; on an empty process it fails and the adapter respawns instead) |
| `gui` | `photocraft <file>` (`PLUMCRAFT_PHOTOCRAFT_APP` overrides) |

## Nodes

| Family | Nodes |
| --- | --- |
| Document | `pc.open`, `pc.new` (file.new), `pc.inspect` (document.inspect, query), `pc.flatten` (layer.flattenImage) |
| Image | `pc.image_size`, `pc.crop`, `pc.rotate` (template over image.imageRotation.*: 90cw, 90ccw, 180, flips) |
| Adjust | `pc.auto` (template: autoTone/autoContrast/autoColor), `pc.brightness_contrast`, `pc.levels`, `pc.hue_saturation`, `pc.exposure`, `pc.black_white`, `pc.invert`, `pc.posterize`, `pc.color_lookup` |
| Filter | `pc.gaussian_blur`, `pc.unsharp_mask`, `pc.add_noise`, `pc.oil_paint`, `pc.clouds` |
| Analysis | `pc.detect_faces` (query, `faces` JSON output) |
| Export | `pc.export_png` (IMAGE), `pc.export_jpg` (IMAGE, quality), `pc.export_pdf` (PDF, not memoised), `pc.export_psd` (`PSD`, viewer-less) |
| Advanced | `pc.run`: any command under filter., image.adjustments., image.imageRotation., image.rotation., image.auto*, image.crop/trim/imageSize/canvasSize/mode./revealAll/detectFaces, layer., select., shape., type., path., edit.fill/stroke/clear/transform./contentAwareFill/contentAwareScale/fade, document.inspect |

`pc.run` denies: smart-object and video-layer commands (file I/O), shape presets and contour libraries (app libraries), font and default-type-style commands, path/layer-style copy/paste (clipboard), `layer.exportAs`, `layer.quickExportAsPng`, `layer.copyToDocument`, and every allowed id containing a pack-policy word (delete, send) even though those are in-document edits.

Demo: New (1024×768) → Clouds → Color lookup (tealOrange) → Oil paint → Export PNG; runs with defaults.

## Assumptions

- Prefix `pc.`; doc wire `photocraft.doc`, ext `pcraft`, media `application/vnd.photocraft` (no registered type; invented).
- Filters and adjustments act on the active layer. An opened JPEG/PNG is one layer, and a new document's background is the active layer, so no layer selection port is exposed (no `[selection]`; PhotoCraft's selection is pixels, not objects). `pc.flatten` merges when a chain has made more layers.
- `pc.new` sends `name = "Untitled"` so the document name does not vary.
- `pc.export_pdf` is `pure = false` as a precaution (PDF writers often stamp dates). Not verified either way.
- `pc.export_psd` uses the plain wire type `PSD` (no viewer).
- No `[thumb]`: the adapter's thumb wants a size query whose reply has `artboards[0].width/height`, which PhotoCraft's `document.inspect` does not have (it has top-level `width`/`height`). Export PNG nodes still get thumbs from the PNG itself.
- Skipped `generate`: PhotoCraft has no typed filter catalogue; the 124 `filter.*` commands are reachable through `pc.run`.

## Open problems and adapter changes

1. **close_all on an empty process fails (fixed in round 2).** The coordinator's reset now respawns the process when `close_all` fails or is missing, and skips the reset on a process that has run nothing; the pack has `close_all = { command = "file.closeAll" }`, and `pack_photocraft` passes 3/3 with it (rerun 2026-10-28, round 2). Original problem, for the record: `file.closeAll` (and `doc_close`) error with "not available right now: no document open" when nothing is open, and the adapter runs `close_all` before every cold open, including on a fresh process, where the error fails the node (config). The pack therefore has no `close_all`: a process keeps every document it opened (each `doc_open`/`file.new` makes the new one active, so saves and exports stay correct) until the pool retires it (60 s idle). Adapter change: `actions.close_all.tolerate = ["no document open"]` (an `Action` field listing error substrings that count as success), or make `reset()` treat any error from `close_all` as success when the process is alive. PhotoCraft-side alternative: `file.closeAll` on no documents returns `{closed: 0}`.
2. **Thumbnails.** `ThumbSpec` could take `width`/`height` reply paths (e.g. `size_paths = ["width", "height"]`) instead of the hard-coded `artboards[0]`; then `[thumb] size_command = "document.inspect"` would give PhotoCraft document thumbs. Also the thumb export passes `scale`, which `doc_export` does not take; a `max_side`-style parameter name would be needed.
3. **Byte stability in a warm process.** A fresh process saves the same document to the same bytes, and open → save is the identity, but document and layer ids come from a process-wide counter (even after `file.closeAll`), so the second `New → Clouds` in a warm process saves with ids 3/4 instead of 1/2. Memo results stay correct (same image), but equal documents can have different handles. PhotoCraft-side proposal: number layers per document.
4. Untested: CMYK/Lab/16- and 32-bit documents through the chain; `pc.detect_faces` on a real face (on the corgi it returns `[]`).
5. The smoke run used the default craft root (`pool::default_root("photocraft")`) and the default store.

## Round 2

- `close_all = { command = "file.closeAll" }` added by the coordinator; with the respawning reset, cold opens start from no documents. `cargo test -p plumcraft-craft --test pack_photocraft`: 3/3 pass; `packs_all` passes; clippy clean.
- Still open: thumbnails (`thumb.size_path`, not available tonight) and the per-process id counter (item 3).
