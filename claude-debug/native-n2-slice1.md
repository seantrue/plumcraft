---
title: "N2 slice 1: the selection in the core, and the canvas's commands"
date: 2026-11-19
---

# N2 slice 1: the selection in the core, and the canvas's commands

Branch `native/n2-selection`, cut from `feat/native-n0` at `a0ea44a`. Nothing is committed; the changes are in the working tree for review.

## Summary

The canvas's selection (nodes, collapsed groups, the net selected) and the group entered are now document state in `plumcraft-editor-core`. Every item of `docs/shell-interface.md` 12.3 is a command with an id, label, `enabled` and a run, reachable from `command.run`, `ui.invoke`, `command.list`, the menu model and the C ABI the same way as every other command. The egui editor's clicks, marquee, keys and menus send these commands; nothing a user sees changed (124 menu dumps identical, every existing test passing unedited). Two items stay shell items in the menu model, by design: Align › and Distribute › (they need the sizes the shell measured; slice 3), and a net's Go to Driver or Next Use (the shell resolves them to a `canvas.reveal`).

**Nothing in the gate failed beyond the known local-only failure.** Suite as CI runs it: 1,394 of 1,395 before, 1,406 of 1,407 after, under both b3 and md5 (the one failure is `hidden_params_match_the_baseline`, there before; 12 new tests). The 41 control replies differ only in `command.list`'s nine new rows. No existing test was edited.

## What moved, and where

| File | Added | Removed | What |
|---|---|---|---|
| `crates/editor-core/src/selection.rs` (new) | 425 | 0 | `Selection {nodes, groups, net}`, `Mode`, `SelectionCell` (the shared handle), hostile-input parsing of `selection.set`, `Visibility` (moved from `groups_view.rs`), Select All, what a command made; 5 tests |
| `crates/editor-core/src/canvas_cmds.rs` (new) | 367 | 0 | The nine canvas commands: table, `command.list` rows, params checking, the core part of each, the hand-off to the shell; 4 tests |
| `crates/editor-core/src/document.rs` | 57 | 1 | `Document::selection`, `Document::focus`; `execute` dispatches canvas commands; selection and focus dropped as the graph loses what they named, cleared on `graph.new`/`graph.open`; `group_to_ungroup` (moved from `crates/ui/src/lib.rs`); the Document's `Host::selection`, `menu_facts` and `shell_command` |
| `crates/editor-core/src/invoke.rs` | 37 | 5 | `edit.cut`, `edit.copy`, `edit.paste`, `edit.delete` invoked as menu items; `select_made` (a `selection.set` of what a command made) replaces `Host::select_result` |
| `crates/editor-core/src/control.rs` | 13 | 5 | `command.list` lists the canvas commands; `ui.invoke` accepts them; `ui.inspect`'s `selection` is the core's |
| `crates/editor-core/src/menu.rs` | 18 | 14 | The Edit menu's Cut, Copy, Paste, Delete, Select All, Frame Selection, View › Fit, Node › Search…, the canvas menu and Select Net carry commands |
| `crates/editor-core/src/lib.rs` | 5 | 0 | The two modules |
| `crates/ffi/tests/selection.rs` (new) | 98 | 0 | A C ABI test: select, read back, Select All, Delete, focus, refusals, hostile params |
| `crates/ui/src/edit_view.rs` | 228 | 152 | `Picked` is a handle to the document's selection; clicks and the marquee become `selection.*` actions; Edit actions invoke their commands; `canvas_command` does fit, the palette and reveal; 2 tests |
| `crates/ui/src/groups_view.rs` | 19 | 83 | `Visibility` re-exported from the core; Enter, Leave and the breadcrumb are `canvas.focus` actions |
| `crates/ui/src/net_view.rs` | 32 | 26 | A tag's click, Esc and an empty click are `net.select`; a jump is `canvas.reveal` |
| `crates/ui/src/canvas.rs` | 25 | 10 | Tab, Space, F and a right-click are `canvas.palette` and `canvas.fit` actions |
| `crates/ui/src/lib.rs` | 29 | 18 | The canvas shares the document's selection; `follow_core` shows the document's focus and net; menu activation of the former local items is through commands |
| `crates/ui/src/control.rs` | 9 | 17 | `ui.set {focus, fit, palette}` run `canvas.focus`, `canvas.fit`, `canvas.palette`; `selection` no longer the window's key |
| `crates/ui/src/menu_view.rs` | 18 | 4 | `action` finds canvas commands; the former local items keep their key text (`canvas_item`) |
| `crates/ui/src/asks_view.rs`, `engine_view.rs`, `settings_view.rs` | 6 | 7 | `Host::selection` from the document; `after_command` follows the core; `window_command` runs the shell's part |
| `docs/control-protocol.md` | 23 | 5 | A section on the canvas's commands; `command.list`, `ui.invoke`, `ui.set`, `menu.get`, the C ABI core's `ui.inspect` |
| `docs/shell-interface.md` | 25 | 20 | 12.3 rewritten as what was done and where each runs; 8.13's rows; 12.1; 8.9's focus |

Measured with `git diff --numstat` (new files with `git add -N`, then unstaged again). Rust: 1,386 added, 342 removed (of which tests, inline and the new C ABI test file, about 330 added).

## The commands added

| Id | Label | Where it runs | Reached from (egui) |
|---|---|---|---|
| `selection.set` | Select | core | a click (replace), Shift-click (add), ⌘-click (remove), snarl's marquee (add); `mode: toggle` for scripts |
| `selection.clear` | Select None | core | a click on empty canvas; after Cut and Delete |
| `selection.all` | Select All | core | Edit › Select All, ⌘A, the canvas menu |
| `net.select` | Select Net | core | a net tag's click (toggle), Esc, an empty click, the net menu's Select Net |
| `edit.delete` | Delete | core (runs `node.remove`) | Edit › Delete, Delete, Backspace |
| `canvas.focus` | Enter Group | core state; the shell rebuilds and fits | the group's pencil, the proxy's pencil, the breadcrumb, Leave group, `ui.set {focus}` |
| `canvas.fit` | Fit | shell | View › Fit, F, `ui.set {fit}`, the canvas menu |
| `canvas.palette` | Search… | shell | Node › Search…, Tab, Space, a right-click on empty canvas, `ui.set {palette}`, the canvas menu |
| `canvas.reveal` | Reveal | shell | a net tag's double-click, Go to Driver, Go to Next Use |

Also made reachable as menu items without a new id: `edit.cut`, `edit.copy` (`ui.invoke` fills in the selection with selected collapsed groups' members and raises `clipboard.set`), `edit.paste` (`ui.invoke` raises `clipboard.get`), `frame.add` (already invoked through `Host::frame_selection`). Esc cancelling a run was already `run.cancel`.

**How the shell-bound ones reach the shell.** Through `Holder::shell_command(id, v)`, the hook `execute` already called for every command the shell answers itself (the egui shell's look: `view.theme`, `view.zoom`). The core checks the params and keeps its part first. The Document's own `shell_command` (the C ABI's core, which draws no canvas) refuses `canvas.fit`, `canvas.palette` and `canvas.reveal` with `"… isn't available from this shell"`, the words it uses for `ui.set`. No new mechanism. I did not route them through `Shell::ui_set`: `ui.set {focus}` in the egui shell now runs `canvas.focus`, so the reverse would recurse, and `ui.set` has no reveal.

**Rules the canvas commands keep.** They are not journaled and are not undo steps (Delete's `node.remove` is). They leave `lastError` and the convert offer as they were, as a click did: routing clicks through `execute` would otherwise have cleared the status bar's error on every click. Errors go to the caller; the person-facing "select nodes to … first" comes from `invoke` (`h.say`), as before. The application hooks (`Hooks::before`, `after`) don't see them, as they never saw a click.

## How egui-snarl's selection is bridged, and what it costs

PlumCraft already kept its own selection by graph id (snarl's is private and keyed by an index the canvas rebuilds on every edit). The slice moved that state into the core and changed how it is written:

- `Document::selection` is a `SelectionCell` (`Arc<Mutex<Selection>>`). The egui canvas's `Picked` holds a second handle to the same cell, so it reads the document's selection while it draws (outline, drag moves, sizes) without the document being lent to the canvas.
- Snarl is still asked for its own selection every frame; the canvas compares it with last frame's and sends what it gained as `selection.set {…, mode: add}` (the marquee, snarl's own Shift-click). It never reads snarl's set as the selection, and snarl's outline stays hidden.
- Clicks are hit-tested by the canvas as before (slice 3 moves that) and sent as commands.

Costs:

- A lock on every read. Uncontended (one thread), a few reads per node per frame. Not measured.
- **The outline appears one frame later.** A gesture's command runs after the canvas has drawn (as every gesture's does), so the click's frame still draws the old outline; the canvas asks for a repaint, so the next frame draws the new one. The palette opened by Tab or a right-click, and a fit, also start one frame later. Not visible at 60 Hz, and no kittest noticed.
- `Picked` keeps `set`, `clear`, `set_group`, `absorb` and `retain_existing`, which change the document's selection without a command. Nothing in non-test code calls them; they keep twelve kittests and two unit tests that set `canvas.picked` directly compiling unedited. A reviewer may prefer those tests rewritten to run `selection.set` and these setters removed.
- The canvas's `focus` and `nets.selected` are copies of the document's, refreshed after every command and at the start of every frame (`EditorApp::follow_core`). Kittests read them as fields; nothing in the shell writes them any more except through that refresh (and `Canvas::enter_group`, which unit tests call directly).
- The menu drawing keeps a list of items that were the shell's own (`menu_view::canvas_item`), so their key text stays as it was (`⌘X`, `⌫`, a blank on Frame Selection) rather than egui's `format_shortcut` (`Ctrl+X` in kittests). Without it the menu dumps differed in 10 of 124 files; with it, none.

## Every change in what the control channel or the C ABI does

On both the egui editor's control port and the C ABI unless said:

1. `command.list` has 9 more rows, after the registry's 124: `selection.set`, `selection.clear`, `selection.all`, `net.select`, `edit.delete`, `canvas.focus`, `canvas.fit`, `canvas.palette`, `canvas.reveal`. Same row shape. Decided (open question 2, A); this is the only difference in the 41 recorded replies.
2. `command.run` and `ui.invoke` of those 9 ids run (before: `unknown command`).
3. `ui.invoke edit.cut` and `edit.copy`: before, the session command ran with no params (no nodes); now the selection fills them, and `clipboard.set` is raised (an ask, the script's under `ui.invoke`). With nothing selected: `lastError` "select nodes to cut/copy first".
4. `ui.invoke edit.paste`: before, `edit.paste` ran with no text (an error); now a `clipboard.get` ask is raised.
5. `menu.get`: Edit › Cut, Copy, Paste, Delete (`command: edit.delete`), Select All, Frame Selection, View › Fit, Node › Search…, the canvas menu's four items gained `command`; the net menu's Select Net gained `command: net.select` and `params: {name, group}`. Labels, shortcuts, roles, enabled states unchanged (the egui dumps say so).
6. **C ABI only:** `ui.inspect` gains `selection` (the selected node ids; it was the egui shell's key, so the egui reply is byte for byte the same).
7. **C ABI only:** the document has a selection, so commands that `ui.invoke` fills from it now act: `group.create`, `node.expand`, `node.remove`, `node.duplicate` (the copies are then selected), `group.ungroup` (the selected group, a selected node's group, or the group entered), `frame.add` still says "select nodes to frame first" (the Document has no sizes to measure), and an application's own commands get `nodes` and `group`. Before, the selection was always empty there.
8. **C ABI only:** `menu.get`'s Edit items are enabled by the document's selection (Cut, Copy, Delete, Frame Selection, Align with two, Distribute with three). Before they were always disabled there.
9. **C ABI only:** `graph.new` and `graph.open` clear the selection and the group entered (the egui shell did this itself before).
10. **C ABI only:** `canvas.fit`, `canvas.palette`, `canvas.reveal` are refused with `"<id> isn't available from this shell"`, and set nothing.
11. Egui only, no reply change: `ui.set {focus}`, `{fit}`, `{palette}` now run the canvas commands. Same errors (`no group …`, `focus must be a group id or null`, `palette must be a boolean or a search string`, `fit must be a boolean`), same reply (the canvas state). `ui.set {focus}` now keeps the group entered in the document too.

## The gate

| Check | Before | After |
|---|---|---|
| `cargo nextest run --all-features --no-fail-fast`, `COMMONKIT_HASH=b3` (CI's environment, fresh folders under `claude-debug/native-n2/s1-*`) | 1,394 of 1,395 passed; failed: `plumcraft-craft::params_coverage hidden_params_match_the_baseline` | 1,406 of 1,407 passed; the same one failed |
| The same, `COMMONKIT_HASH=md5` | 1,394 of 1,395; the same one | 1,406 of 1,407; the same one |
| `cargo clippy --all-targets --all-features -- -D warnings` | not run | clean |
| `cargo fmt --all --check` | not run | clean |
| `scripts/check-core-toolkit-free.sh` | not run | ok |
| `scripts/check-hash-gates.sh` | not run | ok |
| `uv run pytest` (CI's environment, b3 and md5) | not run | 224 of 224 under each; bare, without CI's variables, `test_hash_is_what_was_asked` refuses to run (it requires an isolated store root, by design) |
| `uv run ruff check .` | not run | clean |
| `s5_probe.py`, 41 replies in nine states | two baseline runs: 0 differences between them | 2 differences: `command.list` (empty and after a run) 124 rows to 133; the first 124 rows identical; the 9 added are the canvas commands |
| Menu dump (`PLUMCRAFT_MENU_DUMP`, `crates/ui/tests/menu_dump.rs`), 124 files | baseline | identical (after `menu_view::canvas_item`; the first try differed in 10 files, see the bridge's costs) |
| `~/Library/Application Support/plumcraft-editor/prefs.json`, SHA-256 | `9221fa66…a727` | `9221fa66…a727` (probe runs used `PLUMCRAFT_EDITOR_PREFS` under their own folders) |
| Free disk | 36 GB | 22 GB |

New tests: `selection::tests` (5: modes, hostile params, forgetting gone nodes and nets, Select All with groups and focus, shared handles), `canvas_cmds::tests` (4: state changes leave `lastError` and undo alone, invoked Cut/Copy/Paste/Duplicate/Delete fill in the selection, `net.select`, the view's commands checked before the shell is asked), `edit_view::tests` (2: the canvas draws the document's selection and follows `ui.set {focus}` and the palette; a click's command), and `crates/ffi/tests/selection.rs` (1, over the C ABI: select, read back by reply and `ui.inspect`, toggle, add, hostile params change nothing, `ui.invoke selection.all`, Delete and its refusal, a removed node leaving the selection, focus and Select All inside a group, the view's commands refused, the menus following the selection). None has a millisecond limit.

Probe captures: `claude-debug/native-n2/s1-probe-base`, `s1-probe-base2`, `s1-probe-after`; menu dumps `s1-menus-base`, `s1-menus-after` (the first try, with the 10 differences), `s1-menus-after2`; suite logs `s1-base-{b3,md5}.log`, `s1-final-{b3,md5}.log`; the script `s1-gate.sh`.

## What I did not do

- **Align › and Distribute ›** stay shell items: they need the sizes the shell measured, and the menu model's rule (an item with `params` is `command.run`) can't fill in the selection. Slice 3's `geometry.set` lets the core fill both; then they can be `ui.invoke`d with params, or become commands that read the selection and the geometry.
- **Go to Driver / Next Use** stay shell items: Next Use cycles per tag, a counter the shell keeps (`nets.cycle`). Each jump is a `canvas.reveal`.
- **Keys are not in `shortcut_table()`.** ⌘A, Delete, Backspace, Escape, F, Tab, Space, ⌘X/C/V are recognised where they always were (`edit_input`, the canvas) and dispatch commands. Putting them in the table would make the egui shell's `shortcuts()` fire them in states where `edit_input` holds back (a popup or the palette open), a visible change. The menu model shows them on the items, which is what a native shell binds.
- **No `core.hello`/`caps`.** The contract says the commands come "under the capability `commands.canvas`"; nothing serves capabilities yet, so they are listed unconditionally.
- **No `selected` block for the C ABI.** `ui.inspect`'s `selection` is node ids; the selected groups are only in the egui shell's `groups.selected`, and the net is in no reply but the commands' own (`{nodes, groups, net}`). The scene (slice 2) is where the whole selection belongs.
- **No hit testing, marquee geometry or wire routing moved** (slice 3, as the brief says).
- **The palette is still the shell's state** (open question 4 makes it the core's).
- **The hostile-input sweep** (`crates/proto/tests/hostile.rs`) is the protocol crate's; the new parser is the core's and has its own hostile test (`selection::tests::hostile_params_are_refused`, and the C ABI test's refusals).

## What I am unsure of

- The shared `Arc<Mutex<Selection>>` against a plain field lent to the canvas. The cell kept every existing test unedited; a plain field would mean passing the selection into `Canvas::show` and changing about 35 lines in 12 kittests.
- `selection.clear` leaves the net selected. It matches what the egui editor did (two code paths, with slightly different conditions for "empty canvas"), but a script may expect clear to clear everything.
- `edit.delete` is a new id rather than `node.remove` "given a menu place" as 12.3 proposed, because the Node menu's Remove takes nodes only and the Edit menu's Delete takes selected groups' members too. Unifying them would change one of the two.
- `canvas.focus`'s label is "Enter Group" though it also leaves.
- `selection.set` refuses ids the graph doesn't have, where the old `Picked::set` took anything. Scripts that select before adding would now fail.

## Where the contract disagreed with the code

- 12.3 listed `canvas.fit` as `ui.set {fit: true}`: now it is the other way round (`ui.set` runs the command).
- 12.3 proposed Delete as `node.remove`; it is `edit.delete` (above).
- 12.3 proposed `net.select {key | null}`; a net key on the wire is `{name, group?}` everywhere else (`net.connect`, `net.move`), so `net.select` takes that, and `{}` for none.
- 8.13's "tap on empty canvas clears": it clears nodes and groups; the net has its own path.
- 3.2's `core.hello` capabilities don't exist in the code.
- 12.1 said the Edit menu's canvas items "come from the core too" as items; they also had no commands until now.

## For slices 2 to 4

- **Slice 2 (the scene):** the selection to publish is `Document::selection` (`{nodes, groups, net}`) and the focus `Document::focus`; `selection::Visibility` (now in the core) is what decides which faces and proxies are in the scene. Selection changes are not counted anywhere yet: the scene needs a revision for them (the cell could carry a generation). The egui shell draws the outline from the cell; it should draw from the scene once there is one.
- **Slice 3 (geometry, hit test):** the click's hit test (`edit_view::node_at`) and the marquee (snarl's, bridged by `Picked::gained`) are still the shell's. Once the core hit-tests, `ui.gesture`'s tap can produce `selection.set` itself and the snarl bridge can go. `frame.add`, Align and Distribute should then take the core's sizes and lose their shell hooks (`Host::frame_selection`, `EditOp::Align/Distribute`). `canvas.fit` needs the fit rect (8.9) to become a view request a native shell can read.
- **Slice 4 (`ui.gesture`, conformance):** the gesture table (8.13) can now be written against commands that exist; the conformance suite should cover the refusals of the view's commands on a shell without a canvas, and that canvas commands leave `lastError` alone.
- `ui.set`'s `zoom` and `pan` are still the window's; the view domain (8.9) is where they go.
