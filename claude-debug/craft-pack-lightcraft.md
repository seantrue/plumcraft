# LightCraft pack (Worker A, 2026-10-28)

## Status

**Blocked** (by the pre-made decision on non-file documents, plus one correctness hazard). No built-in pack was added: every `packs/<id>.toml` becomes an editor engine, and this one would give wrong results on branching graphs. A ready draft is at `claude-debug/lightcraft_draft.toml` (untracked): 13 curated nodes plus `lc.run`; the smoke test (`plumcraft-craft-engine --craft claude-debug/lightcraft_draft.toml --smoke claude-debug/craft-mcp/corgi.jpg`) passes every node except `lc.run`, whose default command (`crop.aspect`) needs a parameter. With the adapter change in item 1 below it can go into `crates/craft/packs/lightcraft.toml` as it is, with a test and demo modelled on `pack_photocraft.rs`.

## What LightCraft offers

- `lightcraft-cli mcp [--compact]` is headless, and its library lives in memory (`library.info` → `persistent: false`) unless `--library DIR` is given, so the maintainer's catalog is never touched. There are no automation roots: the craft is unconfined.
- Tools: `command_list` (376 commands, each with a `params` doc), `command_run {id, params}`, `import {paths}` (a string is accepted as well as a list; "the first imported photo becomes active"), `export {path, format?, longEdge?, …}` (`format` takes png|jpeg|tiff|…, so the adapter's added `format` works), `get_develop`, `set_develop`, `render_photo`, …
- A photo can be opened and exported without a persistent catalog: import → develop.* → export. That part works.

## Why it is blocked

1. **No single-file document.** Edits live in the catalog record, not in the file. The candidates for `actions.save` all fail: `export format=original` writes two files (a byte copy of the original plus a `.xmp` sidecar, and the adapter adopts one file); `export .dng` is raw-only ("corgi.jpg: DNG export needs a raw photo"); `photo.saveMetadataToFile` writes a sidecar next to the original, which for us is a file in the CAS. The draft therefore bakes: the document is a full-size 16-bit TIFF with `metadata = "none"` (verified: a reimported TIFF starts from default settings). That is a correct but destructive model: raw latitude (white balance, highlight recovery) is only available to the first node after Open.
2. **No safe reset, and imports dedup.** There is no "close the photo" for an in-memory library: the only way to empty it is `photo.delete` (to Recently Deleted) followed by `library.emptyRecentlyDeleted` (catalog-only; "files on disk stay"). I did not use them: deletion commands are outside what a pack should run, and the environment's safety check refused the probe that tried them. Without a reset, photos accumulate per process, and `import` deduplicates **by path and by content** (`duplicates: [{existing: 1, reason: "path"|"content"}]`) without making the existing photo active. So when a process has already edited the photo for document A (a transform ran warm on it) and a later node needs A again (a branch), the reimport is a duplicate and the node edits the record that already carries the other branch's edits. That is silently wrong output, so the draft is not shipped.

## Adapter changes needed

1. **`pool.respawn_on_reset = true`** (new `PoolSpec` field): where `reset()` would run `close_all`, discard the leased process and start a fresh one instead (`Lease::discard` + spawn). LightCraft starts in milliseconds (start + `initialize` + one call ≈ 6 ms in the probe), and a fresh process has an empty in-memory library, so every cold open imports into a clean library and becomes the active photo. Warm chains are unaffected. This alone makes `claude-debug/lightcraft_draft.toml` correct (it already has no `close_all`). It would also help any craft whose close-all is unsafe or fails on an empty session (PhotoCraft's `file.closeAll` fails with "no document open").
2. **Stateless doc kind** (the coordinator's pre-made decision, for a faithful non-destructive pack): `doc.kind = "stateless"`; the document value is `{source: <image handle>, settings: <develop JSON>}`; Open imports the source and outputs `{source, settings: get_develop}`; a transform imports the source in a fresh state, applies `set_develop {settings}`, runs its command, then reads `get_develop` into the new value; an export imports, applies and exports. This keeps raw data and makes every edit re-editable in the app. It needs fresh state per node too (change 1, or a LightCraft command that clears the in-memory library).
3. LightCraft-side alternative to change 1: a `library.clear` (in-memory libraries only) or `import {…, activateDuplicates: true}` so a duplicate becomes active.

## Draft pack summary (`claude-debug/lightcraft_draft.toml`)

| Manifest | LightCraft |
| --- | --- |
| `command` | `lightcraft-cli mcp --compact` (no `[roots]`: unconfined; in-memory library) |
| `actions.open` | tool `import {paths}` |
| `actions.save` | tool `export {path}` with `format = tiff, longEdge = 0, bitDepth = 16, metadata = none, tiffCompression = zip` |
| `actions.export` | tool `export {path, format}` with `longEdge = 0` |
| `actions.clear_history` | `history.clear` |
| `actions.close_all` | none (see change 1) |
| `doc` | wire `lightcraft.image`, ext `tif`, media `image/tiff`, stem `photo` |
| catalogue | `presets` = `presets.list` (41 presets; `lc.preset`'s choices) |

Nodes: `lc.open`, `lc.light` (exposure, contrast, highlights, shadows, whites, blacks via `develop.set {values}` with `into = "values"`), `lc.presence` (texture, clarity, dehaze, vibrance, saturation), `lc.develop` (any slider ids as JSON), `lc.auto`, `lc.white_balance`, `lc.black_white`, `lc.preset`, `lc.crop`, `lc.settings` (develop.get, query), `lc.export_png`, `lc.export_jpg`; `lc.run` over develop.*, crop.*, presets, masks and spots. Excluded on purpose: anything touching the real library, Apple Photos (`addToPhotos`), model downloads (denoise, faces, segment), printing, slideshows, metadata writes to files.

## Round 2 decision (coordinator): stays blocked

The coordinator (Fable) decided LightCraft stays blocked and the draft stays out of `crates/craft/packs/`. Saving a baked TIFF at every step turns a non-destructive raw developer into a destructive one: raw data is gone after Open, and settings can't be edited again in the app. The respawning reset added in round 2 fixes the dedup hazard, but not this.

## Proposal for the maintainer: a "stateless doc" kind

**Decision asked for:** add a document kind to the adapter where the craft holds no document between nodes. Proposed shape:

- **Value:** the document value is `{source: <handle of the original image or raw, as opened>, settings: <develop-settings JSON>}`. The wire is still `lightcraft.photo`; the blob is small JSON, and its handle is the MD5 of `source` + canonical `settings`, so it is byte-stable and memoises cleanly.
- **Open:** stage the file, `import`, `get_develop` → `{source, settings}` (the defaults, or the settings embedded in a DNG or sidecar).
- **Transform:** on a fresh process (respawn reset), `import` the source and `set_develop {settings}`, then run the node's command, then `get_develop` → `{source, settings'}`. Nothing is rendered. A warm process whose photo already has `settings` skips the import and set.
- **Query:** the same restore, then the command; JSON out.
- **Export:** restore, then the node's export (`export {path, format, …}`). This is the only step that renders pixels, and the only one that needs the image.
- **Open in LightCraft:** write the source plus an XMP sidecar holding `settings` into a temp folder and open that, so the edit stays editable.

The draft's 13 nodes in this model:

| Draft node | Stateless role | Command |
| --- | --- | --- |
| `lc.open` | Open: source + default settings | `import` tool, then `get_develop` |
| `lc.light` | Transform: settings' | `develop.set {values: light.*}` |
| `lc.presence` | Transform | `develop.set {values: effects.*, color.*}` |
| `lc.develop` | Transform (any sliders, JSON) | `develop.set {values}` |
| `lc.auto` | Transform (reads pixels to choose settings; still settings-only out) | `develop.auto` |
| `lc.white_balance` | Transform | `develop.wb {mode, temp?, tint?}` |
| `lc.black_white` | Transform | `develop.treatment {bw: true}` |
| `lc.preset` | Transform | `preset.apply {id, amount}` |
| `lc.crop` | Transform | `crop.set {rect, angle}` |
| `lc.settings` | Query: the settings JSON (or just output the value's `settings`) | `develop.get` |
| `lc.export_png` | Export: the one render | `export {format: png, longEdge: 0}` |
| `lc.export_jpg` | Export | `export {format: jpeg, quality}` |
| `lc.run` | Transform over the develop/crop/preset/mask/spot allowlist | any allowed command |

Adapter changes it needs:

1. `doc.kind = "stateless"` (default `"file"`), with `[doc.state] get = "develop.get"` (or the tool `get_develop`) and `set = { tool = "set_develop", key = "settings" }`. These name how the settings are read back after a node and applied before one.
2. Open for that kind: stage the source in the CAS and keep its handle in the value. Transform and query: restore by `open` (import) + `set`, never `save`; output a JSON blob `{source, settings}` (form `Doc{app}`), hashed canonically.
3. Warm reuse keyed on the value's handle as now. A cold restore always respawns, since a LightCraft import of a known path or content is a duplicate and doesn't become active. Round 2's respawning reset already does this when `close_all` is absent.
4. Export nodes for that kind restore and then run their own action (round 2's export `action`/`command`).
5. "Open in LightCraft": the editor writes the source plus an XMP sidecar into a temp folder (needs an XMP writer, or a LightCraft `photo.writeSidecar {path, settings}` that accepts settings).

No code was written for this.
