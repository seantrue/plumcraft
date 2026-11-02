# Craft pack: mailcraft

Worker D, overnight 2026-10-28.

## Status

**Blocked: the mailbox document cannot be saved by an agent.** Round 2: the pack moved out of the built-in packs to `claude-debug/mailcraft-pack-draft.toml`, so the editor doesn't offer an engine that can't run anything. Note that claude-debug tracks only `.md` files, so the draft exists only in this working tree, and the test skips when it is missing. `crates/craft/tests/pack_mailcraft.rs` loads the draft, keeps every safety check, and asserts that `mailcraft` is *not* a built-in pack. To ship it, move the draft back to `crates/craft/packs/mailcraft.toml` once MailCraft can save (below). The draft parses; with `mailcraft-cli` 0.8.2 on PATH the live catalogue (271 commands) builds all 13 curated nodes plus `mc.run`, nothing dropped. No chain can run: after Open or New the adapter saves the document, and MailCraft refuses: `` `file.saveAs` doesn't take `path` from an agent: the mailbox file is the owner's to open and save `` (also `file.save` and the `save_mailbox` tool, with or without `--allow mailbox` and `--files`). This is deliberate in MailCraft (`crates/engine/src/agent.rs`: `("file.save", "path", FileUse::Refused)`, `("file.saveAs", "path", FileUse::Refused)`), so it is not an adapter bug.

`cargo test -p plumcraft-craft --test pack_mailcraft`: 2 passed (the parse case also checks the safety rules; the live case checks the catalogue and that the save is refused with exactly that message, prints `BLOCKED`, and would run the chain if saving starts working).

## Safety (as the brief requires)

- The server starts only as `mailcraft-cli mcp --sample --allow mailbox` plus `--files <CAS>`: the built-in sample mailbox, never the owner's (no `--file`, no `--connect` to the app). The headless server touches no user folders (checked: no config/data-dir access in `crates/mcp`, `crates/engine`, `apps/mailcraft-cli`).
- `--allow mailbox` is the only class: `Mailbox` ("opens, replaces or saves the mailbox file") is a class of its own in `crates/engine/src/agent.rs`. Never send, delete, accounts, settings or input.
- Nodes list, read, search, mark, flag, categorise, move between folders and export. No compose/reply/forward/send, delete, rules, junk lists, accounts, sync, signatures, options or attachment opening. `mc.run` allows 26 exact command ids (no prefixes, so a new command can't slip in), all `agent: "safe"` in MailCraft's catalogue.
- Message text is untrusted data (MailCraft frames it); it only ever flows out as JSON values.

All probing used `--sample` only. Scratch output: `claude-debug/mailcraft_out/` (an mbox export of the sample's Inbox).

## How it would map

| Manifest | MailCraft |
| --- | --- |
| tools.list | `list_commands` (JSON array; each command has `agent: safe/owner/never`) |
| tools.run | `execute {command, params}` |
| open | `file.open {path}`: through `--files`, the server reads the file and hands the command `data` (works) |
| save | `file.saveAs {path}`: **refused for agents** |
| close_all | none: one mailbox per process; open/new replace it |
| export | `file.exportFolder {path, id?}` → .mbox (works through `--files`; the adapter's `format` key is ignored) |
| roots | `--files` for both; MailCraft takes only the first, so the folder is the CAS and the work folder is outside it (second block) |

## Nodes (13 + run_node)

mc.open (file.open), mc.sample (file.new sample), queries mc.folders (folder.list), mc.messages (mail.list: folder, query, limit), mc.message (mail.get), mc.events (calendar.list), mc.contacts (people.list), mc.tasks (tasks.list); transforms mc.categorize, mc.flag, mc.mark_read, mc.move; export mc.export_mbox (file.exportFolder → MBOX, no viewer).

No PDF/PNG/SVG export: MailCraft has none an agent may use (printing is out by the safety rules).

## What would unblock it

**The MailCraft change (required):** in `crates/engine/src/agent.rs`, the file-use table has `("file.saveAs", "path", FileUse::Refused)` (and the same for `file.save`). When the owner started the server with `--allow mailbox`, let `file.saveAs` take `FileUse::Writes { ext: "mailcraft" }` instead. The server would then write a new self-contained copy inside the `--files` folder (`create_new`: never over an existing file, and never the open mailbox's own path), as it already does for `file.exportFolder`. Without `--allow mailbox` it stays refused. No other MailCraft change is needed: `file.open` already reads through `--files`.

**Plus one adapter change:** a roots form that passes `--files` once, naming the folder that holds both `cas/` and `work/` (below).

## Details (round 1)

1. **Saving (MailCraft side, a proposal; D15 means no craft change now).** Let an agent save a mailbox **copy** into the `--files` folder when the owner passed `--allow mailbox`: e.g. `file.saveAs` gets `FileUse::Writes { ext: "mailcraft" }` instead of `Refused` when `Mailbox` is allowed (a new self-contained file only, `create_new`, never the open mailbox's own path, so the owner's file is still never overwritten). Or a `file.saveBytes`-style command answering `{base64}` as GridCraft has; then the adapter needs a "save by data" action (`Action.data_key = "base64"`: the adapter writes the returned bytes to the file itself).
2. **One folder for both roots (adapter side).** MailCraft's `--files DIR` takes one folder, and `Launch::new` passes `--files <cas> --files <work>` (sibling folders, MailCraft uses the first). Needed in `crates/craft/src/manifest.rs` + `pool.rs::Launch::new`: `Roots.both: Option<String>` (or `roots = { one = "--files" }`) meaning "pass this flag once with the CAS root folder `Cas::root()`, which holds both `cas/` and `work/`". MailCraft's FilesDir accepts subfolders and absolute paths inside it.

With both, the curated chain `mc.sample → mc.categorize → mc.messages / mc.export_mbox` should run as it stands.

Until then a stateless alternative (the brief's "stateless doc" decision) would also work: a doc value that is the source mailbox file, with transforms carried as settings and replayed on open; that is a bigger adapter change and I did not fake it.
