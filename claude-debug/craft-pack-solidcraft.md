# Craft pack: solidcraft

Worker D, overnight 2026-10-28.

## Status

**Done** (round 2: PNG export added, `sol.shell` restored). `crates/craft/packs/solidcraft.toml` parses; with `solidcraft-cli` 0.18.4 (built here: `cargo build --release -p solidcraft-cli` in `~/src/artcraft/solidcraft`, 2 min, no source changes) the live catalogue (404 commands) builds all 21 curated nodes plus `sol.run`, nothing dropped, and the demo runs end to end (`cargo test -p plumcraft-craft --test pack_solidcraft`: 2 passed; checks the STL triangle count, the STEP header, the body's volume against the box less the hole and the fillets, and that the design's handle is stable across runs).

Round 2: `sol.export_png` is an export node with its own action, `action = { tool = "screenshot", path = "path" }`. The adapter sends the path and the widgets (width, height, view, grid, axes) and never `format`. The demo renders 480 × 320 iso, and the test checks the PNG's size. `sol.shell` is back as a node, since packs_all now matches denied words at word boundaries. `solidcraft-cli` is on PATH (`~/bin` symlink to the release build).

## Files

- `crates/craft/packs/solidcraft.toml`, `crates/craft/packs/solidcraft-demo.json`, `crates/craft/tests/pack_solidcraft.rs`
- `claude-debug/solidcraft_probe.py`, `claude-debug/solidcraft_commands.{json,txt}`, `claude-debug/solidcraft_out/`

## Installation

`solidcraft-cli` is not installed in `~/bin`. The pack's `command` is `solidcraft-cli` on PATH with a `hint` saying how to build it, as the brief asks. The test uses, in order: `solidcraft-cli` on PATH, `$PLUMCRAFT_SOLIDCRAFT_CLI`, then `~/src/artcraft/solidcraft/target/release/solidcraft-cli`; it skips when none exists. (Round 2: the coordinator has since put it on PATH, so the editor entry starts.) Before that, the editor entry failed to start without `solidcraft-cli` on PATH (e.g. `ln -s ~/src/artcraft/solidcraft/target/release/solidcraft-cli ~/bin/`). The coordinator may prefer the VectorCraft style (`command` = the absolute target path); I followed the brief. The GUI (`solidcraft`) is not built either; `[gui]` points at `solidcraft` on PATH with a hint.

## How it maps

| Manifest | SolidCraft |
| --- | --- |
| tools.list | `list_commands` (JSON array of `{id, label, panel, params, enabled, …}`) |
| tools.run | `execute {command, params}` |
| open | `doc.open {path}` (.solidcraft, STEP, IGES, FreeCAD, Rhino, 3MF, STL, OBJ, glTF) |
| save | `file.save_as {path, cache: false}` |
| close_all | none: one design per process; `file.new` and `doc.open` replace it |
| export | `file.export {path, format}` (stl, step, 3mf, obj) |
| roots | `--allow-path` for both read and write (SolidCraft's only file flag; it also allows its recovery folder, a private temp folder and the open design's folder) |
| volatile | `path`, `index`, `ms` (replies carry build times) |

## Nodes (21 + run_node)

| Id | Kind | Command |
| --- | --- | --- |
| sol.open | open | doc.open |
| sol.new | new | file.new (name `document`) |
| sol.parameter | transform | parameters.change (name, expression) |
| sol.box | transform | solid.box (length, width, height as expressions; corner; operation) |
| sol.cylinder | transform | solid.cylinder |
| sol.sphere | transform | solid.sphere |
| sol.fillet | transform | solid.fillet (edges as points; fit reduce) |
| sol.chamfer | transform | solid.chamfer |
| sol.shell | transform | solid.shell |
| sol.combine | transform | solid.combine |
| sol.move | transform | solid.move |
| sol.mirror | transform | solid.mirror |
| sol.pattern | transform | solid.pattern.rectangular |
| sol.appearance | transform | appearance.assign |
| sol.measure | query | inspect.measure |
| sol.inspect | query | document.inspect (measure) |
| sol.export_png | export (own action) | screenshot tool → IMAGE (viewer) |
| sol.export_stl / _step / _3mf / _obj | export | file.export → STL / STEP / 3MF / OBJ (no viewer) |
| sol.run | run_node | solid., sketch., surface., construct., parameters., timeline., component., joint., appearance., inspect., model., sheet., plastic., select., explode., material assign/list/create/edit, document inspect/units/metadata; denies every command that reads or writes a path (sketch.insert_svg, *.export_dxf, export_bend_table, timeline.export_script, component.insert_design) and the clipboard-style timeline.copy/paste |

Demo: New → Set parameter `width = 40 mm` → Box (length `width`) → Cylinder cut (base `[width / 2, 15, 0]`) → Fillet four top edges → Appearance → Render PNG, Export STL, Export STEP, Measure. Changing the parameter node's expression rebuilds the chain.

## Assumptions

- Node ids use the `sol.` prefix (`sc.` is SoundCraft's).
- Widgets for lengths are STRING so they take expressions over parameters; SolidCraft parses numbers given as text.
- **Saves without the model cache** (`cache: false`): the cache (`cache/model.json`) records each feature's build time in ms, so cached saves differ run to run. Without it the file is the timeline alone and reopening re-evaluates it (slower on big designs, fine for node chains). Proposal for SolidCraft: leave `ms` out of `cache/model.json` (or zero it under `SOURCE_DATE_EPOCH`), then the pack can drop `cache = false`.
- Body names default to `Body1`/`Body2`: SolidCraft names bodies in creation order, so the defaults suit a fresh design; chains with several bodies set them.
- Sketch-based features (sketch → draw → finish → extrude) are not curated nodes: a sketch is several commands with ids that flow between them, which fits a script better than a chain of nodes. They are reachable one command at a time through `sol.run` (the active sketch is part of the saved design), and a "Run script" node over `batch` would be the better fit later.

## Adapter changes needed

- Round 1 asked for a per-node export action; the coordinator added it in round 2 (`action` on an export node, no `format` sent), and it covers the PNG.
- Still open (minor): a `[thumb]` for documents would need a size source other than `artboards[0]` (`thumb.size_path`) and the screenshot action. It is not needed, because the PNG export has its own thumb.
