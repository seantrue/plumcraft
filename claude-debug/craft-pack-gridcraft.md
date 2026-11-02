# Craft pack: gridcraft

Worker D, overnight 2026-10-28.

## Status

**Done.** `crates/craft/packs/gridcraft.toml` parses; with `gridcraft-cli` 0.13.0 on PATH the live catalogue (355 commands) builds all 19 curated nodes plus `gc.run`, nothing dropped; the demo runs end to end in `crates/craft/tests/pack_gridcraft.rs` (`cargo test -p plumcraft-craft --test pack_gridcraft`: 2 passed).

## Files

- `crates/craft/packs/gridcraft.toml`
- `crates/craft/packs/gridcraft-demo.json`: New (grades sample) → Sort by Average desc → Insert chart → Export PDF, and Read range from the sort.
- `crates/craft/tests/pack_gridcraft.rs`
- `claude-debug/gridcraft_probe.py` (generic MCP stdio probe, reused for the other crafts), `claude-debug/gridcraft_commands.{json,txt}` (the command catalogue), `claude-debug/gridcraft_out/` (probe output).

## How it maps

| Manifest | GridCraft |
| --- | --- |
| tools.list | `list_commands` (a bare array of `{id, label, menu, params, enabled}`; `params` is prose, VectorCraft grammar) |
| tools.run | `execute_command {command, params}` |
| open | `file.open {path}` (xlsx, xlsm, xls, xlsb, ods, csv, tsv, txt, json) |
| save | `file.saveAs {path}` → .xlsx |
| close_all | `file.close {force: true, document: true}` |
| export | `file.saveAs {path}`; format by extension (.pdf, .csv, .html); the adapter's extra `format` key is ignored |
| doc | `gridcraft.doc`, `.xlsx`, stem `workbook` |
| gui | `gridcraft <file>` (env `PLUMCRAFT_GRIDCRAFT_APP`) |

## Nodes (19 + run_node)

| Id | Kind | Command |
| --- | --- | --- |
| gc.open | open | file.open |
| gc.new | new | file.new (sample: budget, sales, grades; unset = blank) |
| gc.add_sheet | transform | home.insertSheet |
| gc.set_cell | transform | cell.set |
| gc.write_range | transform | range.setValues |
| gc.replace | transform | edit.replace (all) |
| gc.number_format | transform | home.numberFormat |
| gc.format_as_table | transform | insert.table |
| gc.sort | transform | data.sort |
| gc.remove_duplicates | transform | data.removeDuplicates |
| gc.subtotal | transform | data.subtotal |
| gc.pivot | transform | insert.pivotTable |
| gc.chart | transform | insert.chart |
| gc.read | query | sheet.read → `values` |
| gc.evaluate | query | formulas.evaluate |
| gc.inspect | query | document.inspect |
| gc.export_pdf | export | file.saveAs .pdf → PDF (viewer) |
| gc.export_csv | export | file.saveAs .csv → CSV (no viewer) |
| gc.export_html | export | file.saveAs .html → HTML (no viewer) |
| gc.run | run_node | allow list: cell., range., home., data., table., chart., pivot., slicer., sheet., formulas., pageLayout., the safe insert.* and edit.* commands, view.freeze*, review notes/comments/statistics |

## Assumptions

- **The document is .xlsx.** GridCraft has no other native format; its .xlsx round-trips charts, tables, pivots, comments and conditional formats (probed: a pivot and two charts survive save → close → open). Two saves of one workbook are byte-identical, so handles are stable without `SOURCE_DATE_EPOCH` (the test checks it). PDF export is byte-stable too, so every node is `pure`.
- **Unconfined.** `gridcraft-cli mcp` has no automation-root flags, so `[roots]` is omitted (noted in the pack header). run_node keeps out every command that takes a path (`insert.picture`, `data.getData`, `data.fromTextCsv`, `pageLayout.background`), plus `file.*`, `automate.*` (scripts), `link.open`, clipboard (`edit.copy/cut/paste`), `window.*`, `view.*` other than freezing panes, and printing (`file.print`). `pageLayout.` is allowed: it sets page setup stored in the workbook (print area, margins), not printing.
- Nodes act on the active sheet; a range can name another sheet (`Sheet2!A1:C3`). `gc.pivot` and `gc.add_sheet` leave their new sheet active, so a following export shows it.
- No `[thumb]`: GridCraft has no artboard-size query and no PNG export of a sheet.
- No `generate`: there is no typed catalogue that maps to nodes.

## Left out

- Chart image export: GridCraft has no PNG/SVG export of a chart or sheet (only PDF of the active sheet); PDF is the viewable export.
- Conditional formatting, validation, Goal Seek, Solver, scenarios: available through `gc.run`; their parameter objects are large and better as JSON there.
- `screenshot` (desktop app only).

## Open problems / adapter changes

1. Resolved in round 2: `file.close` fails when no workbook is open (after a failed open), and the adapter now replaces the process when `close_all` fails. No workaround in the pack; the test still passes (2 passed).
2. Exports use `file.saveAs`, whose prose only names `{path}`, so PDF options (`sheets: all`, `fitToPage`, `range`) of `file.exportPdf` aren't widgets. An adapter that allows a per-node export command (`node.command` on an export node overriding `actions.export`) would let `gc.export_pdf` use `file.exportPdf` with `sheets`/`fitToPage` widgets.
