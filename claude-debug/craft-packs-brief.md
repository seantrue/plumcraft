# Overnight: craft packs and editor entries for every artcraft app

Started 2026-10-28 by the maintainer's request (they are asleep): "Build packs and entries for all the artcraft applications. Use up to 4 agents. Make reasonable assumptions. If you can't complete a pack/entries, make a note as to why, and move on to another app." A Fable session coordinates: answers questions the maintainer would have answered, gives guidance, and signs off.

## What already exists (done by the main session before the workers started)

- `crates/craft` (`plumcraft-craft`) is the generic craft MCP adapter: an artcraft app's MCP server as PlumCraft nodes, generic over one TOML manifest per craft. Read `crates/craft/src/manifest.rs` (the manifest schema, the source of truth), `crates/craft/packs/vectorcraft.toml` (the full reference pack: 25 curated nodes, generated effect nodes, `vc.run`), `crates/craft/packs/soundcraft.toml` (a minimal pack), `docs/research/14-craft-mcp-adapter.md` (the design, especially 3.10 on manifests and 2.4 on how nodes were chosen), and `crates/craft/tests/` (how packs are tested; `common/mod.rs` has helpers).
- **Packs are data-driven now.** `crates/craft/build.rs` turns every `crates/craft/packs/<id>.toml` into a built-in pack, and `packs/<id>-demo.json` (if present) into its demo workflow. No Rust edit is needed to add a craft.
- **Editor entries are automatic.** `apps/plumcraft-editor` builds its engine list from the built-in packs: `plumcraft-editor --engine <id>` (mode 2, its own process, by default; `--inproc`, `--http`) and an entry per pack in the Engine menu, labelled with the manifest's `name`. A pack without a demo opens an empty workflow.
- **`[roots]` is optional.** cadcraft, filmcraft, gridcraft, lightcraft and mailcraft's `mcp` servers have no `--automation-read-root`/`--automation-write-root`; omit `[roots]` for those (the craft is then unconfined; say so in the notes).
- Standalone: `cargo run -p plumcraft-craft --bin plumcraft-craft-engine -- --craft <id|path>` (modes 2/3; `--smoke <image>` runs every node with defaults on an image, VectorCraft-shaped).
- The craft CLIs are in `~/bin` (`<craft>-cli mcp` is the MCP server; `<craft>` the GUI). Sources are in `~/src/artcraft/<craft>`. solidcraft has no CLI installed, but its source has `apps/solidcraft-cli` and `crates/mcp`: build it there (`cargo build --release -p solidcraft-cli` in `~/src/artcraft/solidcraft`) and point the manifest's `command` at `solidcraft-cli` on PATH with a `hint` saying how to build it; for tests, use the built binary via an env override if it isn't on PATH, and skip the live test when absent.

## Your job, per craft

1. Learn the craft's MCP server: run `<craft>-cli mcp` and speak MCP to it over stdio (initialize, tools/list, then its list-commands tool) with a scratch script in `claude-debug/` (name it `<craft>_probe.*`, no collisions). Read the craft's README/AGENTS.md and its MCP tool code for the document model (open/save/close), the command runner tool and its argument names, and progress/cancel.
2. Write `crates/craft/packs/<id>.toml`: id, name, command, roots (if supported), tools, actions (open/save/close_all), doc (wire `<id>.doc`, ext, media, app, stem), gui (the desktop app, so "Open in <Craft>" works), and **curated nodes**: a small, useful set (aim 8 to 20) covering open/new, the craft's characteristic operations (pure-able ones: take a doc, return a doc), and export to a common format (PNG/SVG/PDF/CSV/WAV/MP4/TXT … whatever fits) so results show in the editor's viewers. Add a `run_node` escape hatch with a safe allow/deny list when the craft has a generic command runner. Use `generate` only if the craft has a catalogue that maps cleanly (as VectorCraft's effects do).
3. Add `crates/craft/packs/<id>-demo.json` when you can make a meaningful chain (new/open → operation → export) that runs with defaults or a fixture under `crates/craft/tests/fixtures` (add a small fixture if needed; keep it tiny and licence-clean, e.g. generated).
4. Test: `crates/craft/tests/pack_<id>.rs` — the pack parses; and, when the CLI is installed, the engine starts, its catalogue has the curated nodes, and one end-to-end chain runs (new/open → op → export) with a non-empty, sane output. Run only your own tests: `cargo test -p plumcraft-craft --test pack_<id>`.
5. (Coordinator does this at the end; skip.) Check the editor entry: `cargo run -p plumcraft-editor -- --engine <id> --screenshot claude-debug/<id>-editor.png` (check how `--screenshot` behaves in `apps/plumcraft-editor/src/main.rs` first) and look at the PNG: the engine connected, the palette shows the craft's families.
6. Write `claude-debug/craft-pack-<id>.md`: what the pack covers, the node list, assumptions you made, what was left out and why, open problems. If you could not finish, say exactly why (missing CLI, no headless mode, no MCP document save, adapter limitation and what change it needs) and **move on to your next craft**.

## Rules

- Follow `AGENTS.md` (golden rules: never panic in non-test code; no `unsafe`; no engine specifics in core; hostile input) and `CLAUDE.md`. Scratch scripts and data go in `claude-debug/` (only `.md` files there are tracked; never delete anything there; avoid name collisions — prefix with the craft id).
- **Do not edit shared code**: `crates/craft/src/`, `crates/craft/build.rs`, `apps/`, other crates, other workers' packs or tests, `docs/STATUS.md`, `AI.md`. Four workers share this working tree and its `target/`. If the adapter genuinely cannot express a craft (a missing manifest field, a different reply shape, a document model it can't drive), get as far as you can, write the exact change needed in your notes (file, field, behaviour), and move on. The coordinator will make adapter changes centrally afterwards and may send you back.
- **Do not change the crafts' source** (plan D15: no craft-libs or craft changes now). Record proposals in your notes instead.
- **Outward-facing or destructive commands never become nodes or pass `run_node`'s allow list**: sending mail, network/upload/publish, deleting files outside the work folder, printing and printers, changing app settings or accounts, keychain/signing identities (`sign_*`), model or data downloads (`*_models_download`, place names, OCR models), writes to system libraries (`export_addToPhotos`, importing into the user's real catalog), `app.*` settings, anything a craft marks `agent: "owner"` or `"never"`.
- **mailcraft:** start its server only with `--sample` (its built-in sample mailbox) or a fixture mailbox under `crates/craft/tests/fixtures`; never the maintainer's mailbox. Never pass `--allow send`, `--allow delete`, `--allow accounts` or `--allow options`; `--allow` is permitted only for the class that covers saving the mailbox file, if that is a class of its own (check `mailcraft/crates/mcp/src/backend.rs`). Nodes: list/read/search/transform/export only.
- **One `target/` for everyone** (separate target dirs are not possible). Build and test only `-p plumcraft-craft --test pack_<id>`. **Do not run `cargo run -p plumcraft-editor`**; the coordinator takes the editor screenshots serially at the end (skip step 5). Expect cargo's lock to serialise builds; don't kill another cargo. Live tests skip (never fail) when the CLI is absent, and every live call has a timeout. Delete your `$TMPDIR/plumcraft-craft-tests-*` when done.
- Every pack has at least new/open plus one export to PNG, SVG or PDF where the craft offers one (those have viewers; `IMAGE`/`SVG`/`PDF`/`JSON` are the shared wire types). Other formats (CSV, WAV, MP4, DOCX) use a plain upper-case wire type and are noted as viewer-less.
- No commits. Don't run the whole workspace suite (the coordinator does at the end); keep `cargo clippy -p plumcraft-craft --all-targets -- -D warnings` clean for your files.
- Make reasonable assumptions and write them down; don't stop to ask. Prefer a smaller pack that works end to end over a big one that doesn't.
- Kill any craft processes you start; leave no orphans.

## Assignments

| Worker | Crafts, in order |
| --- | --- |
| A | photocraft, effectcraft, lightcraft |
| B | soundcraft (grow the genericity pack into a real one; keep `sc.new`/`sc.inspect` and the existing tests passing), filmcraft, cadcraft |
| C | wordcraft, deckcraft, designcraft, pdfcraft |
| D | gridcraft, figcraft, solidcraft (start its release build in the background first), mailcraft |

## Report back

A short summary per craft: done / partial / blocked, node count, test result, editor screenshot path, notes file path, and any adapter change you need.

## Pre-made decisions (coordinator)

- **Done** = pack parses, live catalogue lists the curated nodes, one chain runs in `pack_<id>.rs` with sane output, notes written. **Partial** = parses and lists but no end-to-end chain or no export. **Blocked** = the adapter can't open or save the craft's document; write the exact manifest field/behaviour needed and move on.
- **Non-file documents** (e.g. lightcraft's catalog, a directory of snapshot + log): blocked; the needed change is a "stateless doc" kind whose value is the source file and whose transforms carry settings JSON. Don't fake a save action. (First check whether the craft can open a single file and export without a catalog; if so, that's a valid pack.)
- **Explicit document ids** (e.g. pdfcraft's `doc`): if omitting the id does not mean "the active document", that is one adapter change, pre-approved in principle: `actions.open.returns = "<reply path>"` and `doc.id_param = "<name>"`, the id injected on every call. Record it; the coordinator implements it once if two or more crafts need it.
- **No roots** (cadcraft, filmcraft, gridcraft, lightcraft, mailcraft): allowed, unconfined, noted in the pack header comment. mailcraft's single `--files <dir>` cannot cover both CAS and work root; try `roots = { read = "--files", write = "--files" }`, otherwise blocked with that as the change.
- **Spreadsheets, mail, CAD, video projects** fit doc-in → doc-out as long as open/save exist; don't stretch for "pure" nodes, mark `pure = false` when in doubt.
