# N1 slice 8: one menu description and one log for every shell

Branch `native/n1-menu`, working tree only (nothing committed). 2026-11-19.

## Summary

- **The menu model** is `plumcraft_editor_core::menu`: the menu bar and nine kinds of context menu as data (`Item` trees), built from the command registry, the document, what the shell knows (`Facts`) and the platform's `Capabilities`. Shortcuts are data (`{key, mods}`, `primary` = ⌘ on Apple, Ctrl elsewhere); roles mark what a platform places itself.
- **The egui editor draws every menu from it**: the bar, the node, collapsed-group, group-header, wire, input, output, frame and net menus. 124 menus dumped before and after, in eight states: **identical** (`diff -r`: no output).
- **The Engine menu lists what a platform can't start, disabled with the reason** ("this device can't start programs"). On a desktop nothing changes.
- **`menu.get {context?}`** over the control channel and the C ABI, with a generation and a `changed {domain: "menus"}` signal in the C ABI feed.
- **The log is `plumcraft_editor_core::log`**:
  - a ring of 10,000 entries, levels, and an optional bounded file (4 MiB, one rotation);
  - `log.tail` and `log.write` for every shell, and a `log` note in the C ABI feed;
  - every non-test `eprintln!` in the core, ffi, ui and the editor app now goes through it, with the same stderr line;
  - the control token never enters it.
- **The gate holds:**
  - control replies: 0 differences in 41;
  - tests: the same single failure before and after, under b3 (three final runs) and md5 (two);
  - The research software's tests pass with no source change.
- **Also:** an unexpected edit to `docs/BUGS.md` appeared during the session (below, first).

## A file I did not change

`git status` shows `docs/BUGS.md` modified at 17:26. I did not touch it. The edit is one added line under the `mode2_python_kill_9_mid_run_recovers` entry ("Since (2026-11-19): intermittent, not constant… pull request #104…").

- It reads like the controller's own ledger entry.
- No source file changed that I did not change.
- I left it alone and carried on: it is outside my files, so nothing could be overwritten.
- I re-checked `git status` before each final run. Nothing else appeared.

Please confirm the edit is yours.

## The menu model

### Item shape

`menu::Item` (JSON camelCase; empty and default fields left out):

| Field | Meaning |
|---|---|
| `id` | The command id, or, for an item the shell does itself, the command it will become (`edit.cut`, `selection.all`, `canvas.fit`, `canvas.palette`, `rename.node`, `wire.slice`, …); a submenu's path (`menu:File`, `node.add:/Text`) |
| `label` | The words shown |
| `command`, `params` | What activating it runs: `ui.invoke {command}` without params, `command.run {command, params}` with. No command: the shell does it, named by `id` (until N2 gives it a command) |
| `enabled`, `reason` | `reason` is the disabled item's tooltip (registry `enabled` errors, refused hostings, uninstalled doc apps) |
| `checked` | Toggles (`run.auto`, `view.runColors`), Animation Speed, radios, choices, swatches |
| `shortcut` | `{key, mods?}`, mods among `primary`, `ctrl`, `shift`, `alt` |
| `role` | What a platform places itself (below) |
| `hover` | The enabled item's tooltip |
| `items`, `submenu` | Children; `submenu: true` even when empty |
| `separator` | A separator |
| `look` | How a self-drawing toolkit shows it: `plain` (default), `check`, `radio`, `choice` (a selectable label in a row), `selected` (a selectable button), `note` (weak line), `text`, `dim`, `row` (label and choices side by side), `section` (children inline, scrolling) |
| `color` | `#rrggbb` for a frame swatch's label |
| `strong` | The engine in use |

### Roles

- **Commands:**
  - `about` (`app.about`), `preferences` (`app.settings`), `quit` (`app.quit`);
  - `new`, `open`, `save`, `saveAs`;
  - `undo`, `redo`;
  - `cut`, `copy`, `paste`, `delete`, `selectAll` (the Edit menu's canvas items).
- **Menus:** `fileMenu`, `editMenu`, `viewMenu`, `helpMenu`.
- **Reserved:** `windowMenu` and `fullScreen` are in the enum but never emitted. There is no Window menu or full-screen command today; a native shell adds its own.

### Shortcuts

- `menu::parse_shortcut` (moved from `crates/ui/src/lib.rs`) reads the registry's notation into `{key, mods}`.
  - Keys are checked against a list in the core: letters, digits, named keys, F1 to F35.
  - So no toolkit's key table is needed.
- `menu::shortcut_table()` is what a shell binds, the most specific first (same stable order as before), with `text_field_keeps` for `edit.*`.
- The egui shell converts with `menu_view::to_egui`.
- `plumcraft_ui::parse_shortcut` keeps its signature as a wrapper, so its existing test passes unchanged.

### Contexts

`menu::Context`, parsed from `menu.get`'s `context` (a hostile shape is an error):

- `node {node}`
- `proxy {group}` (a collapsed group)
- `group {group}` (an open group's header)
- `wire {to}` (plus the shell's computed `bend` in-process)
- `input {port}`, `output {port}`
- `frame {frame}`
- `net {net, group?, to?}`
- `canvas`: Add Node…, Paste, Select All, Fit. For touch; the egui canvas still opens the palette on a right-click.

### How it is built

- `bar_titles()`, `bar_menu(session, facts, title)` and `bar()`.
- `context(parts, facts, cx)`. `Parts` is graph, catalogue, runs, doc apps and recent apps, so the canvas can lend them without the session.
- `group_menu`, `frame_menu` and `net_menu` take just the graph.
- `Facts`: selection, selected groups, the canvas centre (Node › Add's `pos`), `live`, `caps`, the application's value editors `(kind, label, help)`.
- Supporting moves to `plumcraft_editor_core::rows`, with `plumcraft-ui` re-exporting at the old paths:
  - `input_rows`, `InputRow`, `RowKind`, `port_label`, `unique_name`, `ProxyOut`;
  - `proxy_outputs`: the collapsed group's output list, taken out of `groups_view::proxy_node`, which now calls it.

### When the revision changes

`menu::Revision` hashes the bar's JSON. The generation goes up only when the hash changes. Each `Document` holds one (`doc.menus`).

- `menu.get` without context replies `{gen, menus}`.
- The C ABI core notes the bar after a turn and pushes `changed {doc, domain: "menus", gen}` when it moved. It does this:
  - only for documents a shell has called `menu.get` on;
  - only when the core is idle (a 50 ms tick with no message), or at least once a second when it never is.
- Why the restriction: noting on every turn made `ffi::control_port a_core_opened_through_the_c_abi_is_driven_over_its_control_port` fail 1 time in 2 under full load.
  - That test races a `run.watch` run against `run.start` and expects `done`, not `cached`.
  - The cause was the extra few milliseconds per turn in a debug build.
  - With the restriction it passed in all three final runs, and in 3 of 3 alone.
- Cost: building the bar is cheap once the Engine menu stops reading every craft's manifest.
  - `engines::refused(caps)` builds only the refused offers.
  - The core caches them per thread, keyed by caps and engine count.
  - Before that fix the dump kittest took 4.6 s instead of 2.5 s.

## Before and after: the menu dumps

- **How they were made.** `crates/ui/tests/menu_dump.rs` is new.
  - It opens every menu as a person does: clicks the bar titles, hovers each enabled submenu two levels deep, right-clicks nodes, captions, wires, frames, group headers and net tags.
  - It writes each to `$PLUMCRAFT_MENU_DUMP/<state>.<menu>.txt`:
    - the accessibility widgets inside the open popups and the bar (role, label, value, disabled, toggled, selected);
    - every painted text there with its colour and position.
  - Both lists are sorted by position, since the canvas's own widgets come in varying tree order.
- **States:**
  - `empty` (no engine), with the canvas right-click;
  - `choices` (engines to choose, none connected);
  - `selection` (two nodes selected: bar, node menus selected and not, an input, a slow node's input, a wire, a frame and its Colour submenu);
  - `named` (a named input, a net tag);
  - `group` (header, member node with Promote, Output and Group submenus, the bar, collapsed proxy);
  - `running` (`run.running = true`);
  - `prompt` (New over unsaved changes, and the bar with the prompt open);
  - `inproc` (text engine compiled in, linked; Engine submenus, node menu);
  - `process` (the process hosting current);
  - `auto` (engines picked automatically).
- **The noise check:** the first version of the dump differed between two runs on unchanged code (the canvas's widgets came in varying order). The sorting fixed it, and two before-runs were then identical.
- **Result:**
  - `claude-debug/native-n1/s8-menus-before/` and `s8-menus-before2/` (124 files each) are identical;
  - so are `s8-menus-after/` and `s8-menus-after2/`, the last taken after the final edit;
  - `diff -r` before against after: empty (`s8-menus.diff`, 0 lines).
- The before dump shows something worth keeping: in a kittest the registry's items show `Ctrl+Z` (egui's `format_shortcut` for an unknown OS) while the Edit canvas items show `⌘X` (a `cfg!(target_os)` in the old code). The egui shell keeps both behaviours exactly (`menu_view::fixed_text`).

## How the egui shell draws from it

`crates/ui/src/menu_view.rs` (193 lines, new):

- `draw(ui, items, Style {tokens, bar, close})` returns the clicked item. It draws each `look` with the widget the old code used: `Button`, ✓ / four-space prefix, `RadioButton`, `Button::selectable`, `Button::selected` with a coloured `RichText`, `weak`, `label`, `horizontal` rows, the Engine list's `ScrollArea`.
- `action(item)` maps a clicked item's command to the registry's `&'static str` for the canvas's `Action`s.
- `open_window(ctx, item)` opens the windows the shell does itself: rename node, group or frame; a promoted parameter.
- Where each menu is drawn now:
  - **Bar** (`lib.rs::menu_bar`): each open menu's model is built only while it is open. A click runs `invoke(command)`, `run_all(action)`, or the shell's own (`canvas.fit`, `canvas.palette`, `EditOp::from_item`).
  - **Shortcuts** (`lib.rs::shortcuts`): `menu::shortcut_table()`, the same three rules.
  - **Context menus:**
    - node and proxy: `canvas.rs::node_menu_items`, both the right-click and the header ⋮;
    - wire, input and output: `canvas.rs`;
    - group header and ⋮: `groups_view.rs::group_menu`;
    - frame and ⋮: `frame_view.rs::frame_menu`;
    - net: `net_view.rs::net_menu`.
- **Removed from `crates/ui`:**
  - `MENUS`, `checked`, `command_menu`, `command_items`, `menu_item`, the old `parse_shortcut` body;
  - `edit_view::edit_menu` (Align edges re-exported from the core);
  - `palette::node_menus`;
  - `engine_view::engine_choices_menu`, `by_engine`, `group_engine`, `scrolling`, `target_label`;
  - `groups_view::{node_menu, mark_item, mark_word, group_items, parameters_menu, proxy_menu}`;
  - the bodies of the canvas's node, wire, input and output menus, and `frame_view::frame_menu`'s body;
  - the duplicate `port_label` copies, `input_rows`, `InputRow`, `RowKind`, `unique_name`, `ProxyOut`, `SWATCHES` and the save labels (now re-exports).
- **Kept:** `groups_view::MenuCtx`, still used by the promoted-parameter window.

## The Engine menu without capabilities

- Refused hostings come from `engines::refused(caps)`. Each is added to its engine's group after the app's own choices, unless the app already lists it.
  - Without auto, each is a disabled radio with the reason as its tooltip.
  - With auto, an engine nothing here can start is one disabled item.
- `Document` gains `caps` (desktop by default); the C ABI sets it from its config's `platform`.
- Tests:
  - `menu::tests::the_engine_menu_lists_what_this_device_cant_start_disabled_with_why`:
    - desktop: unchanged, every radio enabled;
    - no `subprocess`: every hosting listed, `text/process` disabled with "this device can't start programs", `text/inproc` enabled;
    - auto: Audio one disabled item;
    - an app's own one-choice list gains nothing on a desktop.
  - `ui::menu_dump::the_engine_menu_shows_what_this_device_cant_start_disabled`: the egui menu, drawn.
  - `ffi::menus_log::a_platform_that_cant_start_programs_lists_process_engines_disabled_with_why`: through the C ABI.

## `menu.get` and its tests

- **Dispatch:** in `control::now` (`crates/editor-core/src/control.rs`), so every shell has it: the egui control port, `plum_request`, a C ABI core's control port.
- **New `Shell` methods**, both with defaults, so no implementor breaks:
  - `menu_facts()`;
  - `menu_revision()`.
- **Overrides:** `Document` and `EditorApp` override both. The egui `EditorApp` gives its real selection and canvas centre.
- **Documented** in `docs/control-protocol.md` (methods table, and the C ABI section's feed notes).
- **C ABI tests,** `crates/ffi/tests/menus_log.rs`, `the_menu_bar_follows_the_document_and_invoking_an_item_runs_its_command`:
  1. File, Edit, View, Node, Run come first.
  2. `edit.undo` is disabled on a fresh document with a reason, role `undo` and shortcut `{key: Z, mods: [primary]}`; `app.quit` has role `quit`.
  3. The same `gen` is returned twice. After an edit the feed's `changed` says `menus` moved and undo is enabled.
  4. `ui.invoke` with undo's `command` empties the graph.
  5. `command.run` with Animation Speed's `command` and `params` sets its check mark.
  6. A node's context menu gives `node.duplicate` with params; running it duplicates the node.
  7. Seven hostile `menu.get`s are refused (a string context, an unknown kind, a missing id, a bad port, a 100,000-character id, an extra field, an array); then a good one is answered.
  8. A missing node's menu has no items.
- **Real editor:** `claude-debug/native-n1/s8_probe.py` drove `menu.get`, `log.write` and `log.tail` over the real editor's control port. Problems: none.

## The log

- **Entry:** `{seq, time, level, source, message, doc?}`.
  - `seq` counts from 1 per process; `time` is Unix seconds (f64).
  - Sources: `core`, `editor`, `ffi`, `engine.<id>`, `shell`.
- **Ring:** 10,000 entries (`log::CAPACITY`); dropped entries are counted.
- **Levels:** `error`, `warn`, `info` (the default), `debug`, `trace`.
  - The egui editor reads `log_level` from its preferences (read through `Prefs::extra`, so `ui.inspect`'s `prefs` block is untouched).
  - The C ABI config takes `log: {level}`.
  - The environment variable is **`PLUMCRAFT_LOG`**: new, since no existing `PLUMCRAFT_*` name was about logging (`PLUMCRAFT_TRACE_STARTUP` is a switch for two trace lines and stays as it was). It wins over both.
- **File:** `dirs.logs` (or `log: {dir}`) in the C ABI config writes `plumcraft.log` there, at most 4 MiB, then moved to `plumcraft.log.1`, one kept. The egui editor writes no file; stderr is its sink, as before.
- **Bounds:** a message is cut at 4,096 characters with `…`; control characters other than newline and tab become U+FFFD; a source is cut to 64 characters of `[a-z0-9._-]`.
- **Secrets:** `log::redact(token)` (secrets of 8 characters or more) replaces the token with `‹redacted›` in the ring and the file. The editor's and the C ABI core's control servers register their token before printing. The stderr line the user reads still shows it, as before.
- **Feed:** the C ABI feed carries `"log": {"seq": N, "worst": "warn"}` when there are entries past the last note.

### What replaced which `eprintln!`

`log::say(level, source, line)` prints the identical line to stderr and keeps it.

| File | Lines (topic) | Level, source |
|---|---|---|
| `crates/editor-core/src/prefs.rs` | Preferences not saved | warn, `core` |
| `crates/ffi/src/core.rs` | Control channel port, token file, token (redacted in the log) | info, `ffi` |
| `crates/ui/src/value_editors.rs` | A value editor's kind taken | warn, `editor` |
| `crates/ui/src/playback.rs` | Audio output error | warn, `editor` |
| `crates/ui/src/lib.rs` | `PLUMCRAFT_TRACE_STARTUP`'s first frame and title (still gated) | info, `editor` |
| `crates/ui/src/lib.rs` | Screenshot write errors (2) | error, `editor` |
| `apps/plumcraft-editor/src/lib.rs` | Control port, token file, token (redacted), window closed, engines stopped | info, `editor` |
| `apps/plumcraft-editor/src/lib.rs` | Registration, worker, manifest problems, `--engine`, choose, bundle, control args, workflow read, app icon, eframe errors (11) | error, `editor` |

### Left as they are

- `crates/editor-core/src/headless.rs`: the `run`, `check` and `tidy` commands' own output for users (10 calls).
- `crates/session/src/commands.rs:331`, "the id is taken": the session cannot depend on the core. A follow-up could route it through a hook.
- Everything in test modules.

### Where engine stderr goes

- **Process engines:** stderr is piped into a 64 KiB tail in `crates/host/src/process.rs`. It is shown in the banner when the engine dies (`EngineState::EngineDown.stderr_tail`).
- **Spawned HTTP engines:** stderr is inherited (`crates/host/src/http.rs:124`), so it goes to the editor's own stderr.
- **What I did (contained, no change to `crates/host`):** `Document::drain_engine` now logs a transition to down or unavailable at `warn`, source `engine.<id>`, with the state text and the stderr tail.
- **Not done:** live capture of a running process engine's stderr. That needs the host to forward its tail, a host change, which I wasn't to touch.

### `log.tail` and `log.write`

- **`log.tail {after?, since?, level?, source?, limit?}`** → `{entries, next, dropped}`.
  - `limit` is 1 to 5,000 (default 500); `source` is a prefix.
  - Unknown fields and bad shapes are errors.
- **`log.write {level, message}`** → `{seq}`.
  - Source `shell`, with the document's id.
  - Unknown fields are errors, so a shell can't claim `source: core`.

### Log tests

- **Core** (`crates/editor-core/src/log.rs`, serialised among themselves):
  - order and filters;
  - hostile (40,000+ characters, control characters, a source of junk, a flood of 10,050 that moves the ring and counts drops);
  - a secret never kept;
  - the file bounded with one rotation, and a folder under a file refused;
  - request checking.
- **C ABI:** `menus_log::the_log_holds_both_sides_is_bounded_and_never_holds_the_token`.
  - The control-channel line is in the log; a made-up token's line ends `‹redacted›`.
  - `log.write`'s line comes back with level, doc and time; the feed carries a `log` note.
  - A 1 MiB message with escape and NUL is bounded and printable.
  - 300 `log.write`s that contain the token, plus bad params.
  - **The token appears nowhere in `log.tail` or in `plumcraft.log`.**
- **Real editor** (`s8_probe.py`):
  - the log holds `plumcraft-editor: control channel on 127.0.0.1:…`, `…control token: ‹redacted›` and the probe's line;
  - stderr still prints the token line;
  - no `ui.inspect` change (below).

## Control-reply diff

`s5_probe.py` and `s5_diff.py` with their masking. The before binary was hard-linked before any change (`s8-bin-before/`), the after binary into `s8-bin-after/`.

- **First captures** (`run1`, `run2`):
  - 9 differences, of two kinds, both of the capture and not the code:
    - 7 were `popouts[0].on_top`, the known window-focus artefact;
    - 2 were the engine switch failing in both, with "no engine binary at …/s8-bin-{before,after}/…", because the text engine binary wasn't beside the hard links.
  - I put `plumcraft-text-engine` beside both and captured again rather than explain them away.
- **Recaptures** (`run3`, `run4` of each):

| Comparison | Replies | Differences |
|---|---|---|
| before run 3 against before run 4 (noise) | 41 | 0 |
| before run 3 against after run 3 | 41 | 0 |
| before run 3 against after run 4 | 41 | 0 |
| before run 4 against after run 3 | 41 | 0 |
| before run 4 against after run 4 | 41 | 0 |

The engine switch succeeded in these (`36-switch-reply.json`: `ok`, mode `process`, connected). `ui.inspect` is unchanged; the new methods are additive.

## Tests

The CI command, `COMMONKIT_STORE_MIN_FREE_GB=2`, a fresh run folder each, logs in `claude-debug/native-n1/s8-*.log`.

| Run | Ran | Passed | Failed | Skipped | Failing |
|---|---|---|---|---|---|
| Baseline b3 (`s8-base-b3`) | 1,378 | 1,377 | 1 | 12 | `plumcraft-craft::params_coverage hidden_params_match_the_baseline` |
| Trial b3 (`s8-try1-b3`) | 1,395 | 1,393 | 2 | 12 | the same, and `ffi::control_port …driven_over_its_control_port` (`cached` for `done`) |
| Final b3 1 (`s8-final1-b3`) | 1,395 | 1,394 | 1 | 12 | the same one |
| Final b3 2 (`s8-final2-b3`) | 1,395 | 1,393 | 2 | 12 | the same, and `control_port` again |
| Final md5 (`s8-final-md5`) | 1,395 | 1,394 | 1 | 12 | the same one |
| **After the fix (C ABI menus noted only when asked for, and only when idle):** | | | | | |
| Final b3 3 (`s8-final3-b3`) | 1,395 | 1,394 | 1 | 12 | the same one |
| Final b3 4 (`s8-final4-b3`) | 1,395 | 1,394 | 1 | 12 | the same one |
| Final md5 2 (`s8-final2-md5`) | 1,395 | 1,394 | 1 | 12 | the same one |

- **Test names:** no test is gone. 17 are new in every final run:
  - 12 core unit tests: `log::tests` 5, `menu::tests` 6, `rows::tests` 1;
  - `plumcraft-ffi::menus_log` 3;
  - `plumcraft-ui::menu_dump` 2.
- **`values_never_wait_for_an_engine`** passed in all runs, the baseline included.
- **`control_port`:** a race in that test, made likelier by my first version of the menus signal (above). It didn't fail after the fix (3 of 3 full runs, 3 of 3 alone), but I can't prove it never fails on the baseline under load.
- **Python, as CI runs it:** 224 passed under md5 and under b3; `ruff check`: clean.
- **Gates** (`check-core-toolkit-free.sh` checks the two core crates): all clean.
  - `cargo fmt --all --check`;
  - `cargo clippy --all-targets --all-features -- -D warnings`;
  - `scripts/check-hash-gates.sh`: ok;
  - `scripts/check-core-toolkit-free.sh`: ok.

## Existing test files touched

None.

- The new test files are `crates/ui/tests/menu_dump.rs` and `crates/ffi/tests/menus_log.rs`, plus unit tests inside the new `log.rs`, `menu.rs` and `rows.rs`.
- `crates/ui/src/lib.rs`'s inline `shortcuts_parse` test is unchanged and passes. It uses `command_specs` through `use super::*`, so that import is now `#[cfg(test)]` in the module's header, outside the test module.

## Preferences

SHA-256 of `~/Library/Application Support/plumcraft-editor/`, before the first run and after the last:

| File | Before | After |
|---|---|---|
| `prefs.json` | `9221fa662e648228030c4af3ca118330e0e6a4a950c1db4206dd66a688aca727` | `9221fa662e648228030c4af3ca118330e0e6a4a950c1db4206dd66a688aca727` |
| `trusted.json` | `a10d6fc822e72d77cbf715c89bde4fa809897db6cde6ccdf7be80341464f4704` | `a10d6fc822e72d77cbf715c89bde4fa809897db6cde6ccdf7be80341464f4704` |

Every real-editor run set `PLUMCRAFT_EDITOR_PREFS` under `claude-debug/native-n1/`:

- `s5_probe.py` sets it under its own output folder;
- `s8_probe.py` under `s8-probe/run1/`;
- The research software's tests under `s8-probe/`.

The only log file written was under the scratch root (`ffi-log/logs`).

## The research software

- **The copy:** `claude-debug/native-n1/s8-<app>-check/`, from slice 7's copy (its `.cargo/config.toml` patches every PlumCraft crate to this tree), with the research software's sources re-synced from its checkout (`62513d7`). The repository was not touched.
- **No source change:** `cargo check --tests` exits 0.
- **Tests pass:** `editors` 7, `open_recipe` 2, `views` 5, `recipe` 3, `identity` 2, `stored` 5 (`tests-1.log`).
- **Its menus:** a scratch test in the copy, `apps/<app>/tests/s8_menus.rs`, registers the research software's commands and editors.
  - It writes the model (`menus/model-bar.json`, 8 menus). Its own items are in their places: File › Clean Up Edit Files and Publish Style to virtimp.com, Group › Export Group and Deliver Group to the Palette.
  - It opens each egui menu and checks that each of the 48 top-level items the model lists is drawn, enabled exactly as the model says (`menus/drawn.txt`).
- **Not done: a before dump of the research software's menus.** It would need a second build of every PlumCraft crate at the old HEAD, several GB in the one `target/`. The generic before and after dumps cover the mechanism its menus go through: registry items, value-editor items in the node menu.

## Differences from `docs/shell-interface.md`

The brief named sections 8 and 15; the menus are in section 12.

- **12.1:**
  - submenus are `{id, label, items, submenu: true}`, not `{title, items}`;
  - items always carry an `id` (absent from the contract);
  - `look`, `color`, `strong` and `role` are new;
  - `form?` isn't built: invoking already asks.
- **12.2:** shortcuts are `{key, mods}` as the contract has it. The brief said `{key, modifiers}`; I kept the contract's `mods`.
- **12.3:** the canvas items are in the menu model with no `command` (their `id` is the proposed command), as the contract says, until N2 registers them. `command.list` is unchanged.
- **12.4:**
  - kinds built: `node`, `proxy`, `group`, `wire`, `input`, `output`, `frame`, `net`, `canvas`;
  - the window-only actions are items with no command: rename, slice, name editors, Go to Driver, Select Net, Draw All as Wires;
  - the wire context takes `to` only (its `from` is read from the graph), and `bend` only in-process.
- **The watched value** ("Open with…" in the Watched panel) is not in the model yet. Its item list is small (recent apps, Choose an App…) and it is still drawn by `watched.rs`.
- **7.2:** the `menus` domain is in the C ABI feed's `changed`. There is no `state.get` yet.
- **15:**
  - entry fields are `{seq, time, level, source, message, doc?}` (the brief's), not `{seq, t, level, target, text}`;
  - `log.tail` takes `after` (and `since` as an alias), not only `since`;
  - its reply is `{entries, next, dropped}`, not `{lines, next, dropped}`;
  - the unified log, logcat, the `log` config callback and Export Diagnostics (`app.diagnostics`) are not built;
  - the file is `plumcraft.log`, 4 MiB, one kept, as specified.
- **Golden rule 1's hostile sweep** lives in `crates/proto/tests/hostile.rs`, which can't reach the core. The new parsers (`Context::parse`, `log.tail` and `log.write` params) have their hostile cases in the core's and the FFI's tests instead.

## Inventory for N1's exit review: logic still in `crates/ui` and `apps/plumcraft-editor`

| What | File | Rough lines | Blocks N2? |
|---|---|---|---|
| Selection, marquee, click rules, drag moves, ⌘-drag reroute, align and distribute sizes, copy, cut, paste at the pointer, Esc cancels a run | `edit_view.rs` (664), `canvas.rs` | several hundred | **Yes**: N2's selection, `selection.*` commands, the Edit items' commands |
| Hit testing, wire geometry and routing, bend points, fit transforms, measured sizes for Tidy, reveal and highlight | `canvas.rs` (1,922), `wire_route.rs`, `run_view.rs` | several hundred | **Yes**: the scene, `geometry.set`, hot calls |
| Group focus (enter, leave, breadcrumb), proxy input rows (`proxy_node`'s inputs), visibility | `groups_view.rs` (875) | about 150 | **Yes**: the scene's groups (the outputs moved this slice) |
| Net tags: drivers, drags, drops onto pins, Go to Driver, Select Net; slice labels | `net_view.rs`, `slice_view.rs` | about 300 | **Yes**: scene and commands (`net.select`, `canvas.reveal`) |
| Wire and port hover previews: 250 and 500 ms timers, the 512-display cache | `wire_view.rs` | about 200 | Yes for touch (contract 8.10); can wait on desktop |
| The status bar's assembly (cells, what shows when) | `lib.rs` `status_bar` | about 130, mostly drawing | N2 (`status` domain) |
| The Watched panel's syncs: pane, compare, pop-outs, `sync_media`, `cache_displays`, `rerun_pending`; the compare computation | `lib.rs` `pump_runs`/`sync_media`/`sync_compare`, `popouts.rs`, `compare_view.rs` | about 100, plus the views' own | N2 (viewer state as commands, 13.2) |
| Viewer-local state: grid format, table sort, text wrap and find, PDF page, log scale; viewer registry | `viewers/*`, `array_view.rs`, `viewers/mod.rs` | about 150 of state | Can wait (contract open question 5, then N3/N4 per viewer) |
| The in-place spectrogram for short clips | `audio.rs` `AudioView::sync` | about 60 | Can wait (media service `Want::Spectrogram`) |
| Appearance commands, theme pump, prefs adoption, retheme | `settings_view.rs::window_command`, `lib.rs` `pump_theme`/`use_prefs`/`retheme` | about 50 | Can wait (theme into the core with `theme.get`) |
| Run animation pump and plan | `lib.rs` `pump_run_anim` (23), `run_anim_plan` (18) | about 40 | Can wait (model in the core already) |
| Close over unsaved changes, prompt mirror | `lib.rs` `handle_close` (about 20), `asks_view.rs::sync_asks` (15) | about 35 | Can wait (the core's asks do the logic) |
| Shortcut dispatch: text-field deferral (`after_typing`), consume rules | `lib.rs` `shortcuts` | 35 | No: the table and rules are the core's; the deferral is toolkit glue |
| Rename and name editors; group parameter window | `rename_view.rs`, `net_view.rs` `NameEdit`, `groups_view.rs` | about 150 | Can wait (`text` asks, contract 11.2) |
| Value-editor registry (process-wide, egui) and Watched "Open with…" menu | `value_editors.rs`, `watched.rs` | about 60 of logic | Can wait (native kits register their own) |
| Windowless startup in the windowed crate: argument parsing, manifests, choose, demo, read workflow, `serve_worker`, `start_control`, `editor_starting` | `apps/plumcraft-editor/src/lib.rs` `run` and helpers | about 400 | Can wait. It blocks only a non-egui desktop binary (plan 4.4) |
| Control server wiring and egui `Shell` additions (`ui.set`, `ui.audio`, screenshots, `inspect_shell`) | `crates/ui/src/control.rs` | about 300 | `ui.set` palette and focus to core state in N2; the rest is the shell's |

## Lines of code

From `git diff --numstat` and `wc -l` of the new files.

| Area | Added | Removed | Notes |
|---|---|---|---|
| `crates/editor-core/src/menu.rs` | 1,448 | 0 | New, about 145 of them tests |
| `crates/editor-core/src/log.rs` | 429 | 0 | New, about 100 of them tests |
| `crates/editor-core/src/rows.rs` | 174 | 0 | New, moved from `canvas.rs` and `groups_view.rs` |
| `crates/editor-core` other | 91 | 1 | `control.rs` (dispatch, `Shell` defaults), `document.rs` (`caps`, `menus`, engine-down log), `engines.rs` (`refused`), `lib.rs`, `prefs.rs` |
| `crates/ffi/src/core.rs` | 131 | 10 | Log config, file, feed note, token redaction, menus signal |
| `crates/ffi/tests/menus_log.rs` | 231 | 0 | New |
| `crates/ui/src/menu_view.rs` | 193 | 0 | New |
| `crates/ui/src` other | 224 | 969 | Menus drawn from the model, duplicates gone; `crates/ui/src` is 553 lines smaller (17,070 to 16,517) |
| `crates/ui/tests/menu_dump.rs` | 406 | 0 | New |
| `apps/plumcraft-editor/src/lib.rs` | 31 | 16 | `say`, level from prefs, token redaction |
| `docs/control-protocol.md` | 4 | 0 | Three methods, the C ABI feed notes |

## Disk

- Free space: 41 GB at the start (40 at the baseline), 38 GB at the end.
- Scratch `claude-debug/native-n1/s8-*` is 377 MB:
  - each run folder's `store` and `craft` were removed after its log was read;
  - the logs, test lists, dumps, both binaries, the probes' replies and the copy of the research software are kept.
- No extra target directory; the research software built into the workspace `target/`.

## What the docs should say

- **`docs/STATUS.md`:** N1 slice 8 is done, and N1's code slices with it.
  - Every menu the editor draws is the core's model (`plumcraft_editor_core::menu`), drawn by the egui shell, and given to any shell by `menu.get`.
  - The Engine menu lists what a platform can't start.
  - One log (`plumcraft_editor_core::log`, `log.tail`, `log.write`, `PLUMCRAFT_LOG`, an optional file).
  - Test counts 1,378 → 1,395, with the same single local failure.
  - The `control_port` flake risk, and its cause.
- **`docs/research/24-native-shells.md`:**
  - **Section 16:** a slice 8 bullet with the above, including:
    - the 124-menu identical dump;
    - the shortcut text quirk;
    - the idle-only menus signal and why;
    - `Capabilities` now on `Document`;
    - engine stderr reaching the log only when an engine goes down.
  - **Section 9:** the logging bullet as built: ring and file and `log.tail` yes; the unified log, logcat and Export Diagnostics not yet.
  - **Section 4.1's last row** is done; section 4.4's `crates/ui` row shrinks.
  - **Section 6, Capabilities:** "listed and disabled with the reason" is now true in the Engine menu.
- **`docs/shell-interface.md`:**
  - **12.1 and 12.2:** the item shape as built (the table above, `id`, `submenu`, `look`, `role`).
  - **12.4:** the contexts as built and their params.
  - **7.1 and 7.2:** the feed's `changed {domain: "menus"}` (sent only after a shell's first `menu.get`) and `log: {seq, worst}`.
  - **15:** entry field names (`time`, `source`, `message`), `after` or `since`, `entries`, the C ABI config `log: {level, dir}` and `dirs.logs`, `PLUMCRAFT_LOG`, `log.write` fixed to source `shell`, the token redaction.
  - **4.2:** `Document.caps` and the Engine menu's use of them.
- **`docs/control-protocol.md`:** done in this slice (`menu.get`, `log.tail`, `log.write`, and the C ABI feed notes).
- **`docs/BUGS.md`:** nothing from me. It may be worth an entry for `control_port`'s `cached`-for-`done` race, which depends on whether the `run.watch` run finishes before `run.start` arrives.
- **`AI.md`:** at commit time.
