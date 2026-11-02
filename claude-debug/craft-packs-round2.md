# Round 2: use the new adapter features

The coordinator made these adapter changes (uncommitted, in `crates/craft/src/`); `cargo build -p plumcraft-craft` is green.

## New in the manifest

1. **Reset respawns.** A cold open on a used process calls `close_all`; when the manifest has none, or it fails (a craft that refuses to close when nothing is open), the adapter kills the process and starts a fresh one. A process that has run nothing is left as it is (`Proc.fresh`). So: a failing `close_all` is now fine, and workarounds for it (deckcraft's batch of four `file.close` with keepGoing) can be simplified if that's cleaner. PhotoCraft now has `close_all = { command = "file.closeAll" }`.
2. **Export nodes with their own action.** On a `kind = "export"` node:
   - `command = "comp.saveFrameAs"` runs that command through the run tool with the export action's path key, or
   - `action = { tool = "export_png", path = "path" }` / `action = { command = "…", path = "…", params = {…} }` runs that tool or command.
   Either way the adapter sends the path plus the node's widgets and **never `format`**. `ext` is still required (`media` optional); `format` is optional when the node has its own command or action. The action's command must exist in the catalogue; its keys are checked like any other.
3. **`[selection] set_key = "clips"`** (or `"handles"`): `set` receives its ids under that key, and ids may be strings (≤ 256 chars) as well as numbers — in `ids:a,b`, the wired `ids` socket, and `find` matches.
4. **JSON Schema params.** A command list whose `params` is a JSON Schema object (or null = no params) is now read: accepted keys come from `properties` (unchecked when `additionalProperties` isn't false), and widget type/min/max/enum/default/description come from the schema.
5. **`[tools] list_resource = "figcraft://commands"`**: the catalogue is read with `resources/read` (first content's text, parsed as JSON) instead of the `list` tool. FigCraft's pack now uses it and its live test runs (615 commands, 22 nodes, none dropped).
6. `solidcraft-cli` is now on PATH (`~/bin/solidcraft-cli`, a symlink to the release build).
7. The safety test `crates/craft/tests/packs_all.rs` matches denied words at word boundaries (so `design.theme`, `solid.shell`, `slide.delete` are fine). Run it after your changes.

Not available tonight (record as follow-ups if you need them): folder documents, per-node `before` commands, `|` union in the params grammar, save-reopen-save, `thumb.size_path`, tool-based non-export nodes, explicit document ids (the coordinator is trying this one for pdfcraft separately — don't touch pdfcraft).

## Same rules as the brief

`claude-debug/craft-packs-brief.md` still applies: only your own packs, demos, tests, fixtures and notes; no shared code; no commits; no editor runs; run only your own pack tests plus `packs_all`; keep clippy clean; update your `craft-pack-<id>.md` notes (status, nodes, what changed in round 2). If a feature doesn't do what you need, write precisely what's missing and move on.
