# PdfCraft pack (worker C, 2026-10-28)

## Status

**Done (2026-10-29, coordinator).** The adapter gained explicit document ids (`actions.open.returns`, `doc.id_param`: the id is kept on the process and injected on every call but the open; reset clears it) and JSON Schema `params` in the catalogue (accepted keys from `properties`; widget type, range, enum and default from the schema; `null` = no params). Export nodes name their own action (no `format`). The draft moved to `crates/craft/packs/pdfcraft.toml` with: `open.returns = "doc"`, `doc.id_param = "doc"`, `pdf.export_text` as `action = { tool = "doc_export_text" }`, and a new `pdf.export_pdf` (`doc_save {full: true}` as an export, output `PDF`, so the editor's viewers show it). Demo: `packs/pdfcraft-demo.json` (open → rotate → watermark → PDF and text). Fixture: `tests/fixtures/pdfcraft-hello.pdf` (1.2 KB, made by PdfCraft: blank page, "Hello"). `pack_pdfcraft` 2/2 live: all 21 curated nodes listed, none dropped; open → rotate 90 → watermark → PDF + text + info (page rotation 90); a second cold run with another watermark closes the first document by id and gives a different PDF; the first chain again gives the first handle (byte-stable). Still open: no New node (doc_create is a tool, not a command; tool-based nodes are a follow-up), no page PNG (craft-side proposal below).

### Before (worker C's first pass)

**Blocked** on the adapter. A full draft manifest is in `claude-debug/pdfcraft-pack-draft.toml` (1 open, 13 transforms, 6 queries, 1 export, `pdf.run`), deliberately *not* in `crates/craft/packs/` (build.rs would ship a built-in engine that cannot save). `crates/craft/tests/pack_pdfcraft.rs` checks the draft parses against today's schema and keeps the safety exclusions; it passes. No demo, no editor entry.

## What PdfCraft does

- Every tool and every command needs `doc`, the id `doc_open` returns (1, 2, 3 … per process, never reused). Leaving it out is an error: `page_rotate: missing argument doc`, `doc_save: missing argument doc`. There is no active document (only `render_preview` defaults when exactly one is open).
- `command_list` gives each command's `params` as a **JSON Schema object** (the tool's `inputSchema`), or `null` for UI-only commands; `command_run {id, params}` runs the command's tool ("unknown params are ignored with warnings"). The tools themselves refuse unknown arguments.
- Saves (`doc_save {doc, path, full: true}`) are byte-stable across processes (checked: blank → add text → watermark, two processes, identical bytes).
- No tool writes one page as a PNG to a path: `page_render` returns the image inline (and refuses `path`), `doc_export_images` writes a folder of `<name>_page_<n>.png`.

## Adapter changes needed (in order of need)

1. **Explicit document ids** (pre-approved in principle by the coordinator): `actions.open.returns = "doc"` (the reply path of the new id; also for a `new` command's reply) and `doc.id_param = "doc"`; the runner keeps the id with the process's active document and injects it into every call: tools at the top level, commands inside `params`; `close_all` and `clear_history` too. Files: `crates/craft/src/manifest.rs` (`Action.returns`, `DocSpec.id_param`), `crates/craft/src/nodes.rs` (`Runner::ensure`, `doc_out`, `action`), `crates/craft/src/pool.rs` (the id beside `active`).
2. **JSON Schema params in the command list.** `read_catalogue` in `crates/craft/src/lib.rs` keeps a command only when `params` is a string, so every PdfCraft command is missing and every command node is dropped ("PdfCraft has no command page.rotate"). Needed: accept an object `params` (store it, and in `accepted()` take the keys from `properties`; widget types and choices could come from the schema too), and keep commands with `params: null` (no parameters).
3. **`format_key = ""`** on the export action (as for DesignCraft): `doc_export_text` refuses the `format` the adapter adds.
4. **Per-node export action** (as for DesignCraft), for Export text, Export Word/HTML (`doc_export_office`), and a page PNG once there is a tool for it.
5. Optional: **tool nodes** (`[[node]] tool = "doc_create"`): a New node (`doc_create` has no command id) and `doc_set_info`, `doc_flatten`, `doc_remove_hidden`, `page_move`, `page_number`, which only exist as tools.
6. Optional: a PdfCraft tool that writes one page as PNG to a path (craft side: `page_render {path}`), for a PNG export with a thumbnail. Proposal only (no craft changes now).

With 1 and 2 the draft's transforms, queries and open/save would work; 3 and 4 bring exports; the PDF document itself is already a PDF a viewer could show (another option: let a doc wire type use the PDF viewer).

## Draft node list

| Family | Nodes |
| --- | --- |
| Document | pdf.open, pdf.info (doc_info), pdf.word_count |
| Pages | pdf.rotate, pdf.delete_pages, pdf.insert_blank, pdf.duplicate_pages, pdf.reverse, pdf.crop (page.boxes) |
| Content | pdf.add_text, pdf.header_footer, pdf.watermark, pdf.background, pdf.links_from_urls, pdf.remove_links |
| Text | pdf.find, pdf.text (text_extract) |
| Standards | pdf.pdfa_verify, pdf.a11y_check |
| Export | pdf.export_text (TXT) |
| Advanced | pdf.run (page, edit, comment-visibility, form-detect, a11y, measure and query commands) |

## Excluded (worker brief and judgement)

Signing and Fill & Sign (`sign.*`, `sign_*`, keychain and Windows identities), printing and printers (`doc_print`, `printers`, `print.*`), OCR (`ocr.*`, model downloads), redaction apply (`redact_apply`; marking is excluded too), JavaScript (`js_*`, `tools.*`, `form.prepare` which sets scripts, `actions.*` which run action sequences), passwords (`protect.*`), and everything that reads or writes files other than the document (`page.insert/replace/extract/split`, `page.combine`, `create.*`, `edit.image/attach`, `comment.stamp/summarize`, `measure.export`, `export.*`, `form.merge_data`, `doc_import_data`, `doc_batch`). `doc.compare` needs a second open document id (meaningless in a node).

## Probe

`claude-debug/wordcraft_probe.py pdfcraft <steps.json>` (shared probe for worker C's crafts); `pdfcraft_steps1.json` shows the id behaviour.
