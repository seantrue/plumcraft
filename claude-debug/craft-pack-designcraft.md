# DesignCraft pack (worker C, 2026-10-28)

## Status

**Done (round 2).** `crates/craft/packs/designcraft.toml` (25 curated nodes plus `dsc.run`), `packs/designcraft-demo.json` (now ends in Export PNG and Export PDF), `crates/craft/tests/pack_designcraft.rs` (2 tests, both pass against DesignCraft, 542 commands). Editor screenshot: left to the coordinator.

### Round 2 changes

- Export nodes use the new per-node `action`, which never sends `format`: `dsc.export_pdf` → `file.exportPdf {path, created: 0}` (reproducible), `dsc.export_png` and `dsc.export_jpg` → the `export_png` tool (`page` 0-based, `scale`). `[exclude]` is gone.
- The test now checks the demo's PNG (612×792, PNG signature) and PDF (`%PDF-`, over 1000 bytes), and every node, exports included, runs with its defaults.
- The old `the_excluded_pdf_export_fails_only_on_format` test is removed.

### Round 1 status (for the record)

Partial: no export, because the adapter always sent `format` and DesignCraft refuses unknown parameters. The two changes asked for below are what round 2 delivered.

## Why no export (the adapter change needed)

DesignCraft refuses unknown parameters, both at the tool level (`export_png`: "unknown argument `format`") and at the command level (`file.exportPdf`: "unknown parameter `format`; the command takes: path, pages, …"). `crates/craft/src/nodes.rs` `Runner::export` always inserts `format` into the export action's parameters. Two changes, both in the manifest schema plus `Runner::export`:

1. **`Action.format_key: Option<String>`** (default `"format"`; `""` = don't send it). DesignCraft: `export = { command = "file.exportPdf", path = "path", format_key = "" }`. With this alone, `dsc.export_pdf` works (take it out of `[exclude]`; the test `the_excluded_pdf_export_fails_only_on_format` prints a note when it starts working).
2. **A per-node export action** (`[[node]] action = { tool = "export_png", path = "path" }`, overriding `actions.export`), so PNG (the `export_png` tool, page and scale) and PDF (`file.exportPdf`) can live in one pack. Also wanted by IDML, EPUB, HTML and PPTX exports (`file.exportIdml`, `file.exportEpub`, `file.exportHtml`, `file.exportPptx`), each its own command.

## How it drives DesignCraft

| Manifest part | DesignCraft |
| --- | --- |
| server | `designcraft-cli mcp --automation-read-root … --automation-write-root …` |
| list / run | `list_commands`; `execute {command, params}` |
| open | command `file.open {path}` (.designcraft, IDML, InDesign .indd/.indt) |
| save | tool `save_document {path}` (native .designcraft zip) |
| close_all | none (see below) |
| export | command `file.exportPdf {path, created: 0}` (refused today, above) |
| selection | `edit.selectAll` (items of the active spread), `edit.deselectAll`, `selection.set {ids}`; no `find` (`find.objects` replies `{ids}`, not `{matches: [{id}]}`) and no colour selection |
| gui | `designcraft <file>` (`PLUMCRAFT_DESIGNCRAFT_APP` overrides) |

## Nodes

| Family | Nodes |
| --- | --- |
| Document | dsc.open, dsc.new (preset, pages, facing, columns, margins, fixed title), dsc.sample, dsc.file_info, dsc.inspect, dsc.preflight |
| Pages | dsc.add_pages, dsc.document_setup, dsc.margins_columns |
| Frames | dsc.text_frame, dsc.shape (rectangle/ellipse/polygon), dsc.equation (LaTeX) |
| Text | dsc.replace (find.change), dsc.replace_font |
| Colour | dsc.swatch, dsc.fill |
| Effects | dsc.opacity, dsc.drop_shadow, dsc.corners |
| Transform | dsc.rotate, dsc.move, dsc.outlines |
| Export | dsc.export_pdf (PDF), dsc.export_png (IMAGE, one page), dsc.export_jpg (IMAGE, one page) |
| Advanced | dsc.run (frames, paths, objects, transforms, type, styles, swatches, tables, text, find, layout, layers, notes, TOC, index, lists …) |

Demo: New (Letter) → Text frame "PlumCraft Quarterly" → Ellipse frame → New swatch Plum → Fill (all) → Drop shadow → Preflight, Inspect, Export PNG and Export PDF.

Test: parses; live, the catalogue lists exactly the curated nodes but the excluded export; the demo runs (frame id, preflight 0 errors, inspect shows the text); a second run gives the same document handle; every doc-taking node runs with its defaults on a page with a text frame and a shape; with `include_excluded`, the PDF export fails only on `format` (or passes and says to un-exclude it).

## Assumptions

- `file.new` takes a `title`; without one the document is `Untitled-N` by the process's count, which made saves differ between warm and cold runs. `dsc.new` sends `title = "Untitled"` by default.
- No `close_all`: `file.close` fails when no document is open, and `batch` stops at the first error (no keepGoing). The server starts with one document, but a failed open would leave none and every later reset in that process would fail. Without `close_all`, `file.open`/`file.new` add a document and make it active, which is all the adapter needs; documents accumulate until the pool retires the idle process. Adapter proposal: a `close_all` that ignores errors (or a "close until none" loop).
- Swatch names include brackets for the built-ins (`[Black]`, `[Paper]`).
- Preset choices are the built-ins `file.presets` lists that I checked (Letter, Legal, Tabloid, Letter - Half, A3, A4, A5).

## Left out, and why

- `place_image` / `file.place` (reads a file; the adapter has no image-into-doc transform), data merge (reads a data source), libraries, snippets, books, links, packaging, printing and printers, preferences, recovery, scripts (`script.run`), XML import, `color.loadProfile`, `style.load`.
- No `[thumb]` (needs `artboards[0]` size; `thumb.size_path` is a follow-up). PNG exports carry their own thumb.
- IDML, EPUB, HTML and PPTX exports (`file.exportIdml/Epub/Html/Pptx`) could now be export nodes with their own commands; not added tonight.
- `close_all` is still absent. Since round 2, a missing or failing `close_all` makes the adapter replace the process, so `close_all = { command = "file.close" }` would also work.
