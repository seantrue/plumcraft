---
title: "Craft packs for every artcraft app — overnight report"
date: 2026-10-29
---

# Craft packs for every artcraft app

## Summary

You asked, before bed on 2026-10-28, for packs and editor entries for all the artcraft applications, with up to four agents, reasonable assumptions, notes where something couldn't be finished, and a Fable session coordinating and signing off.

**Result: 13 crafts are built-in packs with editor entries, and each one runs end to end. 2 are blocked; their drafts are tracked in `crates/craft/drafts/`.** Nothing is committed.

| Craft | Status | Curated nodes | Exports | Confined (roots) | Live tests |
| --- | --- | --- | --- | --- | --- |
| VectorCraft | done (before tonight) | 25 + 116 effects | SVG, PNG, PDF | yes | existing suite |
| PhotoCraft | done | 26 | PNG, JPEG, PDF, PSD | yes | 3 |
| EffectCraft | done | 14 | PNG frame | yes | 3 |
| SoundCraft | done (grown from the genericity smoke pack) | 21 | WAV, MIDI, MusicXML | yes | 3 |
| FilmCraft | done | 20 | MP4, GIF, MOV, WAV, PNG still | no | 4 |
| CADCraft | done | 26 | SVG, PNG, PDF, DWG | no | 5 |
| WordCraft | done | 28 | 6 formats | yes | 2 |
| DeckCraft | done | 26 | 5 formats | yes | 2 |
| DesignCraft | done | 25 | PDF, PNG, JPEG | yes | 2 |
| PdfCraft | done | 21 | PDF, text | yes | 2 |
| GridCraft | done | 19 | PDF, CSV, … | no | 2 |
| FigCraft | done | 22 | PNG, SVG, PDF | yes | 2 |
| SolidCraft | done | 21 | PNG render, STL, STEP, 3MF, OBJ | yes | 2 |
| LightCraft | **blocked** — draft | 13 (draft) | — | no | — |
| MailCraft | **blocked** — draft | 13 (draft) | — | no | safety + refusal |

Every pack also has a `<prefix>.run` escape hatch with an allowlist. Every pack has a demo workflow (`crates/craft/packs/<id>-demo.json`).

## Running one

```sh
cargo run -p plumcraft-editor -- --engine pdfcraft      # or any id in the table, lower case
```

The Engine menu lists every pack in all three hostings. The default is mode 2, its own process. Editor screenshots of every demo, taken in mode 2, are in `claude-debug/editor-shots/`. The PdfCraft and VectorCraft demos start with an Open node that waits for you to choose a file.

## Blocked, and the decisions they need

- **LightCraft.** It has no single-file document: its state is a catalog (a snapshot plus a log). A draft that baked a 16-bit TIFF at every step worked, but the coordinator rejected it, because it turns a non-destructive developer into a destructive one. **Decision for you:** a "stateless doc" kind in the adapter. The value would be the source image, transforms would carry the develop-settings JSON, and one export would render. The proposal, the node mapping and the adapter changes it needs are in `claude-debug/craft-pack-lightcraft.md`.
- **MailCraft.** MailCraft refuses agent saves of the mailbox by design, so no document value can be produced. **Unblocks with a MailCraft change:** with `--allow mailbox`, an agent's `file.saveAs` could write a new copy into the `--files` folder. The adapter would also need a way to pass `--files` once. The tests only ever ran `--sample --allow mailbox`, never your mailbox. Details are in `claude-debug/craft-pack-mailcraft.md`.

## What changed in shared code

- **Packs are data.**
  - `crates/craft/build.rs` turns each `packs/<id>.toml` (and `<id>-demo.json`) into a built-in pack.
  - The editor's `EngineKind::VectorCraft` became `EngineKind::Craft(id)` over those packs.
  - You declined a generic `--engine craft:<name|path>` flag earlier, so each pack instead gets its own plain name.
- **Adapter additions** (`crates/craft/src/`), each decided by the coordinator:
  1. `[roots]` is optional. Five crafts have no confinement flags. Each of those packs says so in its header.
  2. When `close_all` is missing or fails, a reset kills the process and starts a fresh one. A process that has run nothing is left alone.
  3. An export node may name its own `command` or `action` (a tool or a command). It gets the path and its widgets, never `format`.
  4. `selection.set_key` (FilmCraft's `clips`, CADCraft's `handles`), and `string_ids`, so a hex handle such as `101` isn't read as a decimal number.
  5. JSON Schema `params` in command lists. Accepted keys come from the schema, as do widget types, ranges, enums and defaults.
  6. `tools.list_resource`: read the command list from an MCP resource. FigCraft's list tool answers in prose.
  7. Explicit document ids (`actions.open.returns`, `doc.id_param`). The id is kept on the process and sent with every call. PdfCraft needs this.
- **Tests.**
  - `tests/packs_all.rs` checks that:
    - every pack and draft parses;
    - node ids and prefixes are unique across packs;
    - no node, node action, shared action or `run_node` allowlist sends, deletes for good, prints, signs, touches accounts, downloads, changes settings, runs scripts or launches external apps. Matching is by word boundary, so deleting a slide inside a document is allowed.
  - `tests/manifest_features.rs` covers the new fields and the hostile cases.
  - Each craft has its own `tests/pack_<id>.rs`.
- **Editor tests.**
  - The engine-menu counts now follow the pack list.
  - The VectorCraft version label check was loosened from "0.16" to "0." (the installed CLI is 0.18).
  - A race in the VectorCraft kittest's SVG "drawn at" label failed 1 run in 6. It now waits for the label and passed 8 of 8.

## Gates

| Gate | Result |
| --- | --- |
| `cargo test --all-features` | 456 passed, 0 failed; 145 s wall with 13 live crafts |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cd python && uv run pytest` | 52 passed |
| `uv run ruff check .` | clean |

## Bugs found and fixed during the night

- The `set_key` rename dropped `ids` even when no `set_key` was set, which broke VectorCraft's selection. The panic hunt caught it, and it's fixed.
- `ids:101` was parsed as a number, so CADCraft selected the wrong object. Making every id a string whenever `set_key` is set fixed that but broke FilmCraft, whose clip ids are numbers. The fix is the explicit `string_ids` flag, which CADCraft sets.
- FigCraft and FilmCraft both used the `fc.` prefix and shared five node ids. FigCraft is now `fig.`, and a test keeps prefixes unique.
- The safety test first flagged harmless names (`design.theme` matched "sign", `solid.shell` matched "shell"). It now matches at word boundaries and also checks nodes' own actions. That closes a loophole worker B pointed out and declined to use: SoundCraft's score SVG export is `file.print_score`, so it was left out rather than routed around the check.

## Things done to your machine

- **`~/bin/solidcraft-cli`** is now a symlink to `~/src/artcraft/solidcraft/target/release/solidcraft-cli`. Worker D built it with `cargo build --release -p solidcraft-cli`; SolidCraft's source wasn't changed. This is outside the repo, and `update-crafts.sh` doesn't know about it: the other crafts in `~/bin` are copies, this one is a link. Remove it or replace it with a copy if you'd rather install it your usual way.
- **Test scratch** is in `$TMPDIR/plumcraft-craft-tests-*`: 78 folders, some from before tonight. The known issue that the tests leave data behind is unchanged.
- **No craft source** was changed (D15), and no commits were made.

## A deny-list call for you

SoundCraft's score SVG export is `file.print_score`. It's a file export, but the safety test's word `print` catches it, so it was left out rather than routed around the check. Two options: a per-pack exception (for example `deny_except`), or a narrower `print` pattern (for example `print.`, `print_document`, `printer`).

## Proposals for the crafts (not done, D15)

- **WordCraft:** honour `SOURCE_DATE_EPOCH`, and stop bumping `cp:revision` on save-as. Today saves differ from run to run, which churns handles.
- **CADCraft:** write a new drawing's full header on its first save. Until then, `new` and the transforms are `pure = false`. Separately, `select` reports `selected: 1` even for a handle that doesn't exist.
- **PdfCraft:** a `page_render {path}` that writes one page to a PNG, which would give a thumbnail and a PNG export.
- **MailCraft:** the agent save-a-copy change described above.

## Follow-ups for the adapter

- folder documents (SoundCraft's `Audio Files/`)
- per-node `before` commands (WordCraft's caret, DeckCraft's insertion point)
- `|` alternatives in the params grammar (DeckCraft's theme nodes)
- CADCraft save-reopen-save
- `thumb.size_path` (thumbnails for more crafts)
- tool-based nodes (a PdfCraft New node)
- WordCraft `SOURCE_DATE_EPOCH` (a craft change)
- multi-step video export (EffectCraft, FilmCraft)
- LightCraft's stateless doc

## Where things are

- Packs: `crates/craft/packs/`. Drafts: `crates/craft/drafts/`. Tests: `crates/craft/tests/`.
- Per-craft notes: `claude-debug/craft-pack-<id>.md`.
- The worker brief and round 2: `claude-debug/craft-packs-brief.md`, `claude-debug/craft-packs-round2.md`.
- `docs/STATUS.md` has a new entry under Next.
- Note 14 section 3.10's manifest sketch is superseded by `crates/craft/src/manifest.rs`.

## Sign-off

The Fable coordinator reviewed the brief before the workers started and decided each adapter change. It made the calls you would otherwise have been asked for (LightCraft stays blocked; CADCraft's `new` and transforms are not pure; PdfCraft's document ids time-boxed). At the end it checked the tree and signed off. It confirmed that no craft source changed tonight: the 23 changed files in `~/src/artcraft/photocraft` are the 2026-10-26 the research software supporting www.virtimp.com pin, from before tonight. When you ask for a commit, `AI.md` will get its entry, including the defects found and fixed in tonight's own work.
