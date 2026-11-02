# Craft pack: FilmCraft

Worker B, 2026-10-28. Status: **done** (round 2 added the PNG still and selection by clip id).

## What the pack covers

`crates/craft/packs/filmcraft.toml`: build a self-contained FilmCraft project (synthetic demo footage, colour mattes, bars and tone, text and shape graphics), put items on a sequence, apply video effects, Lumetri looks and speed changes, and export the active sequence as MP4, ProRes MOV, GIF or WAV.

- Server: `filmcraft-cli mcp` (0.14.0). **No automation roots**: the craft is unconfined (it can read and write anywhere the user can). Said in the pack header.
- Tools: `command_list`; `command_run {id, params}`. Doc actions are commands: `file.open {path}`, `file.save {path}`, `file.closeAllProjects {force: true}`. Export: `file.exportMedia {path, format, wait: true}` (without `wait` it returns a job id at once and the file isn't there yet).
- Doc: wire `filmcraft.project`, ext `fcproj` (the GUI's documented project extension), stem `project`. Saves are byte-stable (checked with `cmp`), and the project name follows the file stem.
- Selection: `[selection] all = edit.selectAll`, `none = edit.deselectAll`, `set = timeline.select`; effect/look/speed nodes act on `select` (default `all`).
- Catalogues: `effects.list {kind: Video}` (the effect node's choices) and `lumetri.presets` (the look node's choices).
- GUI: `filmcraft <file>` (on PATH), override `PLUMCRAFT_FILMCRAFT_APP`.
- Pool: one process (a render uses every core already; also makes the cold-reopen test deterministic).
- Demo: `filmcraft-demo.json`: New project → Demo footage (Aurora) → New sequence from its item → Text → Lumetri look → Export MP4 (quarter size, first 3 s) and Export GIF.

## Nodes (20 curated + `fc.run`)

| Node | Kind | Command |
| --- | --- | --- |
| fc.new | new | file.newProject |
| fc.sequence | transform | file.newSequence (`fromItem` socket; output `sequence`) |
| fc.settings | transform | sequence.settings |
| fc.inspect | query | sequence.inspect |
| fc.items | query | project.items {recursive} |
| fc.demo_footage | transform | file.importDemoFootage (output `item`) |
| fc.color_matte | transform | file.newColorMatte (output `item`) |
| fc.bars_tone | transform | file.newBarsAndTone (output `item`) |
| fc.place | transform | timeline.place (`item` socket) |
| fc.text | transform | graphics.newText (output `clip`) |
| fc.shape | transform | graphics.newShape (output `clip`) |
| fc.marker | transform | markers.add |
| fc.effect | transform | effects.apply (choices from effects.list), select all |
| fc.look | transform | lumetri.applyPreset (choices from lumetri.presets), select all |
| fc.speed | transform | clip.speedDuration, select all |
| fc.export_mp4 | export | file.exportMedia h264 (wire `MP4`, no viewer) |
| fc.export_mov | export | file.exportMedia prores (wire `MOV`, no viewer) |
| fc.export_gif | export | file.exportMedia gif (wire `GIF`, no viewer; Form::Bytes) |
| fc.export_wav | export | file.exportMedia wav (wire `WAV`, no viewer) |
| fc.export_png | export | own command file.exportFrame {format: png, target: program, time?} (IMAGE: viewer and thumb) |
| fc.run | hatch | allow timeline., clip., effects., lumetri., graphics., sequence., markers., playhead.set/step, edit.; deny list in the pack (files, renders to disk, transcription, clipboard, templates, app defaults) |

Item ids are not fixed (they depend on what the project already holds), so media nodes output the new item's id and `fc.place`/`fc.sequence` take it on a socket.

## Tests

`cargo test -p plumcraft-craft --test pack_filmcraft`: 3 pass, about 2.5 s. (1) parse + demo nodes; (2) the demo chain: all 19 curated nodes pass the catalogue check, MP4 is an ISO file of 48 frames (2 s at 23.976), GIF starts `GIF8`, then a cold reopen of the memoised project from the CAS renders the same 48 frames; (3) every other node and `fc.run` (`timeline.setTrack`) in one chain: MOV, WAV, and the inspect shows the tint effect and the marker; the bars-and-tone item is in the project.

## Assumptions

- Self-contained projects only: demo footage, mattes and bars and tone are `Generator` media stored in the project JSON, so the `.fcproj` is the whole document.
- `graphics.newText`/`newShape` place at the playhead, on a new track above the clips; no node moves the playhead (use `fc.run playhead.set`).
- The effect node offers every `effects.list` id, including intrinsic ones (`motion`, `opacity`, `time_remap`) that `effects.apply` may refuse; that's a config error at run time, not a crash.

## Left out, and why (adapter changes)

- **PNG (and any still) export.** `file.exportMedia` writes stills as a numbered sequence (`x000.png`, `x001.png`, … even for a one-frame range; an empty range is refused), so the adapter's adopt of `x.png` would fail. The single-file still is `file.exportFrame {path, format: png|jpeg|tiff|bmp, time}`, a different command, and the adapter has one `actions.export` per pack. **Adapter change (also needed by SoundCraft):** a per-node export action, e.g. `NodeSpec.action = { command = "file.exportFrame", path = "path" }` for `kind = "export"`, falling back to `actions.export`. With it: `fc.export_png` (format png, a `time` widget in ticks or seconds), giving the editor a viewer and thumbnails.
- **Thumbnails.** `[thumb]` needs `size_command` whose reply has `artboards[0].width/height`; FilmCraft's sequence size is at `sequence.inspect` → `settings` (not checked in detail). With per-node export and a thumb `size_path` field, a PNG frame thumb would work.
- **Importing footage.** `file.import {paths}` would make the project point at files outside the CAS (and FilmCraft is unconfined, so any path works). The open node always runs `file.open`, so opening a movie as a new project isn't expressible either. Same two changes as SoundCraft: an open node with its own command, and a bundle document (or a FilmCraft "collect into project" form: `file.projectManager {mode: collect}` exists but writes a folder).
- **`select = "ids:…"`**: the adapter sends `{ids: [...]}` to `selection.set`; FilmCraft's `timeline.select` takes `{clips: [...]}`. **Adapter change:** `selection.set_key = "clips"` (default `ids`). Until then only `all`/`none` work; nodes that need specific clips can wire `clips` through `fc.run`.
- **No roots**: FilmCraft's MCP server has no confinement flags. A FilmCraft change (D15, proposal only): `--automation-read-root`/`--automation-write-root` like SoundCraft and VectorCraft.
- Not curated (reachable through `fc.run`): transitions, trims, keyframes, captions, multicam, audio mixer, text styling.

## Probe

`claude-debug/soundcraft_probe.py` (generic), scripts `filmcraft_s*.json`, outputs `filmcraft_tools.json`, `filmcraft_commands.json`/`.txt`, scratch media under `claude-debug/fc_work/`.

## Round 2 (2026-10-28, later)

- `fc.export_png`: the per-node export `command = "file.exportFrame"` with `fixed = {format: png, target: program}` and an optional `time` (ticks). It produces one PNG at the sequence size, so it's an `Image` form with a thumbnail. This replaces the "PNG export" adapter request above.
- `[selection] set_key = "clips"`: `select = "ids:<clip>"` and a wired `ids` list now reach `timeline.select {clips}`. This replaces the "`select = ids:…`" request above.
- New test `a_png_frame_and_a_selection_by_clip_id`. It checks that a 320×180 PNG is written (`\x89PNG`). With `select = none`, `effects.apply` fails with a config error. With `select = ids:<the text clip>`, the tint is applied to exactly that clip. `pack_filmcraft`: 4/4 pass, and `packs_all` passes.
- Still open: thumbnails for the project doc itself (needs `thumb.size_path`), importing files (needs folder documents and an open node with its own command), and roots (a FilmCraft change).
