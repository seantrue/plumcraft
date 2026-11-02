# Craft pack: figcraft

Worker D, overnight 2026-10-28.

## Status

**Done** (round 2). The catalogue is read from the MCP resource `figcraft://commands` (`[tools] list_resource`, added by the coordinator), because FigCraft's `list_commands` tool answers in prose. With `figcraft-cli` 0.12.1 the engine lists 615 commands and builds all 22 curated nodes plus `fig.run`, nothing dropped. `cargo test -p plumcraft-craft --test pack_figcraft`: 2 passed. The live case runs the demo through the engine (New → Rectangle → Ellipse → Boolean subtract → Fill → Drop shadow) and checks the PNG (2×, larger than 500 × 400), the SVG (contains the fill #7C3AED), a PDF export (`%PDF`), the CSS from Inspect code (has `box-shadow`), and that the document handle is the same on a second run.

## Round 1 (history)

Round 1 was blocked because `list_commands` is prose, so `read_catalogue` failed with `not a command list`. The coordinator's fix is `tools.list_resource`.

## How it maps

| Manifest | FigCraft |
| --- | --- |
| tools.run | `run_command {command, params}` |
| open | `file.open {path}` (.figcraft, .fig, .sketch, FigCraft/REST JSON, .svg, images) |
| save | `file.saveAs {path}` → .figcraft (zip: document.json + thumbnail.png) |
| close_all | `file.close {force: true}` |
| export | `file.export {path, format, ids: []}`; `ids: []` = the whole current page (default would be the selection) |
| selection | `select.all` / `select.none` / `select.set {ids}`; no find (no `{matches}` reply), no colour select |
| roots | `--automation-read-root` / `--automation-write-root` (supported) |
| env | `FIGCRAFT_NO_SYSTEM_FONTS=1` |
| gui | `figcraft <file>` (env `PLUMCRAFT_FIGCRAFT_APP`) |

## Nodes (22 + run_node)

| Id | Kind | Command |
| --- | --- | --- |
| fig.open | open | file.open |
| fig.new | new | file.new (name fixed to `document`) |
| fig.frame | transform | frame.create (preset or box) → `id` |
| fig.rectangle | transform | shape.rectangle → `id` |
| fig.ellipse | transform | shape.ellipse → `id` |
| fig.text | transform | text.create → `id` |
| fig.set | transform, select | node.set (props JSON spread as keys) |
| fig.auto_layout | transform, select | layout.add |
| fig.align | transform, select | align.{left,hcenter,right,top,vcenter,bottom} |
| fig.tidy | transform, select | arrange.tidy |
| fig.group | transform, select | object.group |
| fig.frame_selection | transform, select | object.frameSelection |
| fig.fill | transform, select | fill.set |
| fig.effect | transform, select | effect.add |
| fig.boolean | transform, select | bool.{union,subtract,intersect,exclude} |
| fig.flatten | transform, select | vector.flatten |
| fig.find_replace | transform | edit.findReplace (select: false) |
| fig.code | query, select | inspect.code (css, html, react, swiftui …) |
| fig.pages | query | page.list |
| fig.export_png | export | file.export png (scale widget) → IMAGE |
| fig.export_svg | export | file.export svg → SVG |
| fig.export_pdf | export | file.export pdf → PDF |
| fig.run | run_node | node., frame., shape., text., vector., bool., layout., align., distribute., arrange., transform., object., fill., stroke., effect., style., select., selection., component., variable., inspect., a few page.* and edit.* ids |

Demo (`figcraft-demo.json`): New → Rectangle (r=24) → Ellipse → Boolean subtract → Fill #7C3AED → Drop shadow → Export PNG (2×), Export SVG, Inspect code (CSS).

## Assumptions

- **Determinism.** With `FIGCRAFT_NO_SYSTEM_FONTS=1`, two separate processes producing the same document write byte-identical .figcraft, PNG and PDF (probed). The document's name follows the save path's stem, which the adapter fixes (`document`), so every node is `pure`.
- **Selection.** FigCraft does not save the selection, so after a cold open nothing is selected; nodes that act on layers take the `select` port (default `all`, the page's top-level layers) so warm and cold runs agree. Creation nodes leave the new layer selected, which is harmless because exports pass `ids: []`.
- `fig.set` spreads its JSON widget into the params (`key = "*"`), so any `node.set` key works.
- No `inspect_document` query: queries must be commands through `run_command`, and FigCraft's document inspection is a tool (`inspect_document`), not a command. `fig.code` and `fig.pages` are the command-backed queries.
- No `[thumb]`: needs `artboards[0].width/height` from a command; FigCraft has none. PNG exports have their own thumb.

## Left out

- Components, variables, styles, prototyping: reachable through `fig.run`; their parameter shapes (ids of components, variable collections) need ids from earlier nodes, which the `id` outputs give but which make poor defaults.
- `image.place`, `file.import`, `file.placeImage` (paths; the open node covers images), plugins (`plugin.*`: code execution, network directory), `file.saveVersion`, collaboration, `edit.copy/paste` (clipboard), `text.link`, `edit.delete`, `page.delete`.

## Open problems

- None known. (`file.close` on an empty process fails, but since round 2 the adapter replaces the process when `close_all` fails.)
