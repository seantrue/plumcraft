# Craft pack: CADCraft

Worker B, 2026-10-28. Status: **done**.

## What the pack covers

`crates/craft/packs/cadcraft.toml`: open a DXF/DWG or start a drawing, draw (lines, polylines, rectangles, circles, polygons, text, linear dimensions, hatches) at explicit coordinates, modify the selection (move, rotate, scale, mirror, rectangular and polar arrays, explode, properties, overkill), make layers, query entities, and export SVG, PNG, PDF and DWG.

- Server: `cadcraft-cli mcp --config-dir ""` (0.14.0; the empty config dir means no settings folder, so user plotters, templates and settings don't leak in). **No automation roots**: the craft is unconfined. Said in the pack header.
- Tools: `list_commands`; `execute {command, params}`. Doc actions are commands: `open {path}`, `saveas {path}` (DXF 2018 by extension), `closeall`. Export: the `export {path, format: dxf|dwg|svg|png|pdf}` command.
- Doc: wire `cadcraft.dxf`, ext `dxf`, media `image/vnd.dxf`, stem `drawing`. Saves are byte-stable (two saves `cmp` equal); SVG, PNG, PDF and DWG exports are byte-stable too, so every export is pure.
- Selection: `[selection] all = selectall`, `none = ai_deselect`, `set = select`, `set_key = "handles"` (round 2). Modify nodes default to `select = all`.
- GUI: `cadcraft <file>` (on PATH), override `PLUMCRAFT_CADCRAFT_APP`.
- Fixture: `crates/craft/tests/fixtures/cadcraft-triangle.dxf` (244 bytes, hand-written ENTITIES-only DXF: three lines and a circle; no licence issue).
- Demo: `cadcraft-demo.json`: New → Rectangle (filleted) → Circle → Hatch (ANSI31 between them) → Text → Polar array ×3 → Export SVG and PNG.

## Nodes (26 curated + `cad.run`)

| Node | Kind | Command |
| --- | --- | --- |
| cad.new | new | new {template: "."} |
| cad.open | open | open |
| cad.inspect | query | drawing.inspect |
| cad.entities | query | entities (outputs count, entities) |
| cad.layer | transform | layer.new (current) |
| cad.line | transform | line |
| cad.pline | transform | pline |
| cad.rectangle | transform | rectang |
| cad.circle | transform | circle |
| cad.polygon | transform | polygon |
| cad.text | transform | text |
| cad.dim_linear | transform | dimlinear |
| cad.hatch | transform | hatch {points, pattern} |
| cad.move / rotate / scale / mirror | transform, select | move / rotate / scale / mirror |
| cad.array_rect / array_polar | transform, select | arrayrect / arraypolar {associative: false} |
| cad.explode | transform, select | explode |
| cad.properties | transform, select | properties.set |
| cad.overkill | transform | overkill (output deleted) |
| cad.export_svg / png / pdf | export | export (SVG, IMAGE, PDF wire types: viewers) |
| cad.export_dwg | export | export dwg (wire `DWG`, no viewer) |
| cad.run | hatch | an explicit allowlist of draw/modify/query commands; deny: lisp, appload, script, setvar, options, aliasedit, clipboard, wblock, bsave, xattach, import, export, plot, publish, share |

## Tests

`cargo test -p plumcraft-craft --test pack_cadcraft`: 4 pass, about 1.5 s. (1) parse + demo nodes; (2) the demo graph: all 26 curated nodes pass the catalogue check; the DXF has the HATCH; the SVG is a Vector form; the PNG an Image with a thumb; a second run from the memo gives the same SVG handle; (3) the fixture opens, rotates, `cad.entities` counts its 3 lines, exports a `%PDF-`; a missing file is a node error; (4) every other node and `cad.run` (`copy`, zero offset) in one chain, overkill removes the duplicates, DWG starts `AC10`, inspect lists the new layer. Clippy is clean for `plumcraft-craft --all-targets`.

## Assumptions

- Coordinates are JSON `[x, y]` widgets (drawing units, y up); a node graph supplies numbers, not picks.
- Arrays are made non-associative, so they save as plain entities and stay editable by later nodes.
- `cad.hatch` takes internal points; a point that isn't inside a closed boundary is a config error from the craft.
- The PNG export uses CADCraft's own size and dark background (2400×1600 in the probe); the `export` command has no size options in its grammar.

## Open problems

- **Open → save is not the identity for a new drawing.** A new drawing's first save lacks header variables that `open` fills in (`$CHAMFERA`, `$CHAMFERB`, …), so a chain that runs warm (the drawing stays in one process) and the same chain run cold (reopened from the CAS) can save different bytes. After one round trip it converges (checked: open → save → open → save is `cmp` equal). Memo results stay correct (keyed by the input handle), but D43's warm = cold is not exact for drawings born from `cad.new`. Fixes: a CADCraft change (proposal, D15: give a new drawing the header defaults `open` would), or an adapter option, e.g. `actions.settle = true`: after a `new` node, save, reopen and save again before hashing.
- **`select = "ids:…"`**: the adapter sends numeric `{ids}` to `selection.set`; CADCraft's `select` takes hex `{handles: ["10A", …]}`. **Adapter change:** `selection.set_key = "handles"` plus string ids (handles are hex strings, not u64). Until then only `all`/`none` work; nodes that need particular objects use `cad.run` with `handles`.
- **No roots**: CADCraft's MCP server has no confinement flags (proposal: `--automation-read-root`/`--automation-write-root`). `open` and `import` also take base64 `data`, which would let a future adapter pass documents without paths.
- No `[thumb]` for drawing docs: it needs `size_command` returning `artboards[0].width/height`; CADCraft's extents are in `drawing.inspect` (`extents`). An adapter `thumb.size_path` field would give drawing thumbnails. PNG exports have thumbs already.
- Not curated (reachable through `cad.run`): arcs, splines, offset/trim/extend/fillet (they need handles and pick points), blocks, tables, constraints.

## Probe

`claude-debug/soundcraft_probe.py` (generic), scripts `cadcraft_s*.json`, outputs `cadcraft_tools.json`, `cadcraft_commands.json`/`.txt`, scratch files under `claude-debug/cad_work/`.

## Round 2 (2026-10-28, later)

- **Memo purity (coordinator's decision):** `cad.new` and all 18 transform nodes are now `pure = false` (memo scope none). Warm and cold saves of a drawing born from `cad.new` differ in header lines, and the memo would see that (D47). `cad.open`, queries and exports stay pure. The parse test asserts this. **Follow-up that would make them pure again:** an adapter save-reopen-save step (e.g. `actions.settle = true`: after a `new` node, save, close, reopen and save before hashing), or a CADCraft change so a new drawing carries the header defaults `open` fills in. Open → save converges after one round trip (checked).
- **Selection by handle:** `set_key = "handles"`. New test `a_selection_by_handle_moves_only_that_object`: 11 line segments take handles 100..10A, the circle is 10B, and `select = "ids:10B"` moves only the circle.
- **Gap in the `set_key` feature (adapter):** in `select = "ids:…"` the adapter parses an all-digit id as a number. CADCraft reads a numeric handle as a **decimal** handle (probe: `select {handles: [256]}` selected handle `100`), so `ids:101` selects 0x65, not 0x101: the wrong object, silently. Fix: with a `set_key`, keep `ids:` entries as strings (or add `selection.id_kind = "string"`). Until then use a handle with a letter, or a wired `ids` list of strings. Separately, CADCraft's `select` reports `selected: 1` for a handle that doesn't exist (`FFF`), so the adapter's "selected nothing" check can't catch a bad handle (a CADCraft proposal).
- The `set_key` request in "Open problems" above is done; the decimal-handle gap replaces it.
- `pack_cadcraft`: 5/5 pass; `packs_all` passes; clippy is clean.
