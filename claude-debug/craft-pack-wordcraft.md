# WordCraft pack (worker C, 2026-10-28)

## Status

**Done.** `crates/craft/packs/wordcraft.toml` (28 curated nodes plus `wc.run`), `packs/wordcraft-demo.json`, `crates/craft/tests/pack_wordcraft.rs` (2 tests, both pass against WordCraft 0.12.0, 648 commands; nothing dropped by the catalogue check). Editor screenshot: left to the coordinator.

## How it drives WordCraft

| Manifest part | WordCraft |
| --- | --- |
| server | `wordcraft-cli mcp --automation-read-root … --automation-write-root …` |
| list / run | `list_commands`; `execute {command, params}` |
| open | tool `open_document {path}` (docx, doc, odt, rtf, md, html, tex, txt) |
| save | tool `save_document {path}` (format from the extension; native doc = `.docx`) |
| close_all | `file.closeDocument {force: true}` (closes the active document; the last one leaves a blank) |
| export | tool `save_document {path}`; the `format` key the adapter adds is ignored by WordCraft |
| selection | `select.all` / `select.collapse`; `set` is `select.all` too (no object ids in a word processor) |
| gui | `wordcraft <file>` (`PLUMCRAFT_WORDCRAFT_APP` overrides) |

## Nodes

| Family | Nodes |
| --- | --- |
| Document | wc.open, wc.new (blank/sample/letter/resume/report), wc.properties, wc.word_count, wc.info |
| Text | wc.set_text (document.setText), wc.replace (edit.replaceAll, `replaced` output) |
| Review | wc.accept_changes, wc.reject_changes, wc.clean (file.inspect remove) |
| Format (whole body via select.all) | wc.paragraph_style, wc.font, wc.font_size, wc.change_case, wc.line_spacing |
| Design | wc.style_set, wc.theme, wc.watermark |
| Layout | wc.orientation, wc.paper_size, wc.margins, wc.columns |
| Export | wc.export_pdf (PDF), wc.export_png (IMAGE, page 1), wc.export_txt (TXT), wc.export_md (MD), wc.export_html (HTML), wc.export_odt (ODT) |
| Advanced | wc.run (allowlist: format., para., table., layout., design., references., text., caret., select., insert.* for in-document objects, a few review/file queries) |

TXT, MD, HTML and ODT are viewer-less wire types.

Demo: New (sample) → Replace "Studio"→"Orchard" → Style set (shaded) → Watermark DRAFT → Export PNG and PDF; Word count off the replace.

Test: the pack parses; live, the engine lists exactly the curated nodes, the demo runs (PNG > 500 px, PDF starts `%PDF-`, > 100 words), a text chain (new → set text → replace → font → upper case → TXT) gives `DEAR PLUM,` / `THANK YOU.`, and every doc-taking node runs with its defaults on the sample document.

## Assumptions

- The warm process keeps caret and selection between nodes; a cold open puts the caret at the start of the body. Curated nodes therefore avoid caret-dependent commands: no "insert text at caret", no table of contents (inserts at the caret, replacing a leftover select-all), no header/footer/page-number nodes (`insert.header` leaves the caret in the header story, so a following select.all would select the header). These are reachable through `wc.run` (memo none) with that caveat.
- Theme choices are the six `design.themes` returns (Craft, Studio, Gallery, Atelier, Darkroom, Sketchbook), hard-coded in the pack; a catalogue would need `design.themes` to reply with an object (it replies with a bare array; `[[catalogue]] path` digs into an object).
- `pool.max = 2`; timeout 300 s.

## Left out, and why

- File commands other than properties/inspect/info (save, saveAs, print, printPdf, encrypt, options, share, templates, versions, recover), mailings, tools/macros, developer controls, view.*, font.add/loadSystem (system), clipboard commands, `insert.picture`/`insert.textFromFile`/`insert.object` (read files), `review.compare/combine` (read files), readAloud.
- No `[thumb]`: the adapter's thumb needs a size query with `artboards[0].width/height`; WordCraft has pages, not artboards.

## Open problems and proposals

1. **Saves are not byte-stable.** `docProps/core.xml` gets the save time and an incrementing `cp:revision`. Round 2 check: WordCraft does **not** honour `SOURCE_DATE_EPOCH`. Two saves made with `SOURCE_DATE_EPOCH=0` still have different `dcterms:modified` times. The source has no env lookup: `crates/engine/src/cmd.rs` `now_unix()` reads `SystemTime::now()`, and `crates/pdf/src/metadata.rs` and `crates/formats/src/odt/writer.rs` do the same. No other variable pins the time either, so the pack has no `[env]`. Proposal for WordCraft: have `now_unix()` honour `SOURCE_DATE_EPOCH` (as VectorCraft does), and don't bump `cp:revision` on a save-as to a new path. Until then document handles differ per run. Memo by inputs still works, and `content` keys are not used.
2. **Caret state across warm runs** (above). Adapter proposal: an optional `actions.normalize` command (WordCraft: `insert.closeHeader`, then `select.collapse` + `caret.docStart`) run after every transform before the save, so the warm state matches a cold open. That would let header/footer/TOC/insert-text nodes in.
3. **Thumbnails.** Adapter proposal: `[thumb]` with a fixed export (`format = "png"`, `scale`) and no size query, or a size query path other than `artboards[0]` (WordCraft `layout` reply has `pageW`/`pageH`).
