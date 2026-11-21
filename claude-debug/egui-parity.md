# egui parity: B45 and the small gaps

Branch `native/egui-parity`, made from `feat/native-n0` at `57adab6`, on the Mac. Nothing committed, pushed or opened as a PR. Date: 2026-11-21.

## The answer

Everything the maintainer chose ("Option 2": B45 plus the palette, double clicks, pop-out actions, Show on Face, Match the System, channel picker) is done in the egui editor and tested in the kittest harness, with one limit each where egui can't do what the Apple apps do: Share shows the value's file in the file manager (egui has no share sheet), there is no drag out (egui and winit can't start a file drag out of a window), and the channel picker is on decoded raster pictures only, not on an array's view pictures. B45 is fixed at its cause. The work found and fixed one defect in the core's gesture rule that affects every shell: a double click on an output pin opened the node's parameters when the first click's watch grew the face. B46 did not reproduce in 510 cancels and stays open. The rename window's title no longer repeats a word.

## What the maintainer chose

Asked how far to bring egui up, with three options, the maintainer chose "Option 2": B45 plus the small gaps (palette, double clicks, pop-out actions, Show on Face, Match the System, channel picker). Not in scope, and not done: egui's viewers following the core's viewer state (the audit's D4), removing the Watched side column, egui reporting rows and header parts to the core (D5), a toolbar, names edited in place. None of the items needed one of these.

## Item by item

| Item | Done | How it works | Verified by |
|---|---|---|---|
| B45 (D1): a hidden row shifts the face | Yes, at the cause | egui built rows with `input_rows` less tucked fields; the scene also leaves out rows `node.face` hides. egui now uses the core's `shown_rows`, so pin i is the scene's row i; a row whose key differs from the scene's draws no field | `kittest_face_rows` (5 tests): drag beside "amount" with `count` hidden; check box beside "flag"; typing beside "with" with `find` hidden on Replace; wires into rows before and after the hidden one; a graph opened with two hidden rows. 4 of them failed before the fix, all pass after |
| Palette offers kinds of engines not started (D3) | Yes | egui fills `offered_defs` (the core's `engines::offered_defs`), draws `startable_defs` through `palette::with_starts` ("· starts the array engine"); the add is held by the core's document (`Document::hold_adds`, new, off for the C ABI core) and runs once the engine connects | `the_palette_offers_a_kind_whose_engine_isnt_started_and_choosing_it_starts_that_engine` (a person's click in the palette, held, added at the palette's place, one step "Add Node") |
| Double clicks by the core's rule (D7) | Yes | egui sends `ui.gesture {kind: doubleTap}`; its own double-click code for faces, titles, wires and bend points is gone; a frame's tab and an open group's band send `name.edit`. Core change: a double tap opens what its first tap hit (`gesture::DOUBLE_TAP`) | `kittest_double_clicks` (6 tests): empty canvas opens the palette there, output pin and wire pop out, collapsed group entered, tab and band rename, a pin that moves under the first click still pops out; the existing `a_double_click_on_a_node_opens_its_parameters` and `a_double_click_on_a_field_in_a_node_is_the_fields` |
| Pop-out actions | Yes, less drag out | Copy, Save As…, Share…, Pin as A, Pin as B on the bar; Copy, Save As and Share invoked for the person (the core raises the clipboard, file and share asks), the pins run `viewer.pin {slot, popout}`. Share: egui answers the core's `share` ask by showing the file in Finder (`open -R`), File Explorer or the file manager. Drag out: not possible in egui | `kittest_popout_bar` (5 tests): every button there; Copy puts "THE CORGI NAPS" on the clipboard; Save As goes to `run.save` (the text engine can't write files, so the core says why); Share shows `value.export`'s file holding the text in full; Pin as A and B pin the pop-out's value and turn Compare on |
| Show on Face | Yes | A box per row in egui's parameters window, `node.face {node, port, shown}`; disabled on a wired input | `show_on_face_in_the_parameters_window_hides_and_shows_a_row_through_the_core` |
| Match the System (D6) | Yes | A Match the System box over the theme cards (`view.theme {name: "system"}`); with no theme in the preferences egui follows the system (the core's default) and writes nothing; a stored theme stays | `a_new_editor_matches_the_system_and_settings_can_turn_it_off_and_on`, `a_stored_theme_is_kept` (system light and dark simulated through egui's input) |
| Picture channel picker | Yes, for raster pictures | The pop-out's channel comes from the core's `viewer.picture` state; the picker sends it; the picture drawn through the core's new `plumcraft_media::raster::isolate`. The Watched panel has no pop-out to keep a channel, so no picker | `a_pop_outs_channel_picker_is_the_cores_viewer_picture` (the core's state, the drawn picture grey 50 for G, a script's `viewer.picture` drawn too); `raster::tests::a_channel_shows_as_grey_and_rgb_drops_alpha` |
| Rename window title (D8) | Yes | "Rename “group 1”" and "Rename “Frame”" | `a_collapsed_groups_node_has_no_node_rename` (updated) |
| B46 (D9) | Not reproduced | 510 cancels, every one honoured | See below; B46 left open with what was learned |

## B45: the cause

`Canvas::sync` (`crates/ui/src/canvas.rs`) made each `CanvasNode`'s rows from `input_rows` and dropped only the fields the node's choice tucks away. The core's scene, which egui reads for each pin's label, pin colour and value text, builds its rows with `rows::shown_rows`, which also drops the rows `node.face` hides. egui's `input_pin` took the scene's row by pin index for the label and the `CanvasNode` row by the same index for the widget and the port it edits, so with one row hidden every row after it showed one port's label beside the next port's field, an extra empty pin appeared at the bottom, and a drag changed the port whose field it was. Snarl's wires are also placed by the scene's pin index, so a wire into a row after the hidden one was drawn into the wrong row. The fix uses `shown_rows` for egui's rows (the same function the scene uses) and, as a guard, draws no field on a row whose key isn't the scene's for that pin.

## A defect found and fixed during the work

The after pictures showed a double click on Upper case's output pin opening the node's parameters, not a pop-out. The first click is a tap that watches the value; the watch reruns, the face's run status widens the header (with the application's fonts "cached · 0.00 ms" is wider than the slot kept for it), and the pin at the face's right edge moves about 20 graph units right. The second click, a double tap, then hit the face's body. This is in the core's rule, so the Apple apps can hit it too. The core now keeps the last tap's hit and a double tap within 0.8 s and the hit distance of it uses that hit (`gesture::DOUBLE_TAP`). The kittest that shows it (`a_double_click_on_a_pin_that_moves_with_the_first_click_still_pops_out`) failed without the change and passes with it.

## B46: what was tried

Scratch drivers in `claude-debug/egui-parity/b46/` (copied into the test folders to run, removed after). A 1.5 s Slow node after a Text node whose text changes each time (nothing cached), cancelled at delays from 0 to 1,000 ms after `run.start`.

| Path | Cancels | Ended cancelled |
|---|---|---|
| egui editor, `run.cancel` by command and by the menu's invoke, alternately | 150 | 150 |
| egui editor, a run already going (Auto-Run's, or another `run.start`) when `run.start` superseded it | 120 | 120 |
| egui's control dispatcher (`ui.invoke run.cancel`, `ui.waitRun`), as the audit sent them | 90 | 90 |
| The C ABI core, `command.run` and `ui.invoke {by: person}` | 150 | 150 |

What was learned: cancel at 0 ms (before the run reached the engine) is honoured; a cancel during a superseded run is honoured. In the audit's sequence the cancel came after 40 frames, the first GPU picture of that process on a software renderer, `ui.inspect` and `menu.get`; if those took more than 5 s the node had already finished, which would explain a `done` at 5,008 ms without a bug. B46 stays open.

## Everything is a command

Nothing egui gained carries out a user action itself: the palette's pick is `palette.choose`, double clicks are `ui.gesture` (and `name.edit` where the tab or band takes the click), the bar's buttons are `value.copy`, `value.saveAs`, `value.share` and `viewer.pin`, Show on Face is `node.face`, Match the System is `view.theme`, the channel is `viewer.picture`. egui's old double-click code (faces and titles to `node.parameters`, wires to `view.popout`, bend points to `wire.route`, the frame's and group's rename windows opened directly) is gone. Two pieces stay egui's: the net tag's double click (to the driver, then each use in turn, richer than the core's driver-only rule) and the answer to a `share` ask (showing the file), which is platform chrome.

## Contract changes

`docs/shell-interface.md`: 4.1 (egui holds an add of a kind whose engine isn't started, `{held, kind, engine, until}`), 8.13 (egui's double clicks; a double tap opens what its first tap hit), 11.2 (egui's answer to `share`, the pop-out bar, no drag out), 12.7 (the palette in egui), 12.15 (B45 and Show on Face in egui), 13 (the channel picker), 14 (Match the System in egui). `docs/control-protocol.md`: `palette.choose`'s held reply in egui. No message type changed: `crates/editor-core/schema/shell1.json` regenerated last and unchanged.

## Conformance steps un-skipped

None. The 23 steps egui skips are `only: rows` (14), `media` (5), `reports` (2), `prefs` (1) and `port` (1); none of the items here reaches them (rows and reports are D5, `media.get` and the port are not egui's control channel).

## The xcframework and `native/apple`

Not rebuilt, and nothing under `native/apple` was changed by this work. The C ABI surface did not change, but the core did in two places the Apple apps will get with their next build of the core: the double-tap rule (`gesture::DOUBLE_TAP`) and `plumcraft_media::raster::isolate` (new, unused by them). `Document::hold_adds` is off for the C ABI core. The Rust conformance drivers through the C ABI and over its control port passed with the new rule. `swift test` was not run: it builds against the existing xcframework, so it would test the old core. `native/apple/PlumCraftPad/device.sh` shows as modified in the checkout; that edit ("the signing team id") is not this work's.

## Gate (Mac)

| Check | Result |
|---|---|
| `cargo fmt --all --check` | Clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | Clean (one lint in the new `isolate` fixed: `as_chunks_mut`) |
| `cargo nextest run --all-features --no-fail-fast` (scratch env) | 1,563 run, 1,562 passed, 1 failed, 17 skipped; the failure is `plumcraft-craft::params_coverage hidden_params_match_the_baseline`, the known local one; no B37 to B40 |
| Conformance: egui (suite and array cases), the C ABI (suite, read half, array cases, control port), the core's own document | All passed |
| `scripts/check-hash-gates.sh` | ok |
| `scripts/check-core-toolkit-free.sh` | ok |
| `uv run pytest -q`, `uv run ruff check .` (scratch env) | 224 passed; all checks passed |
| Schema regenerated | Unchanged |

New tests: 21 (5 in `kittest_face_rows`, 6 in `kittest_double_clicks`, 5 in `kittest_popout_bar`, 2 in `kittest_prefs`, 1 in `kittest_viewer_tools`, 1 in the editor app's engine tests, 1 in `plumcraft-media`), from 1,542 to 1,563.

## Real locations, before and after

Counted with `claude-debug/egui-parity/realcheck.sh` (files and newest modification): the real store `/Volumes/<store>/store`, `~/Library/Caches/com.plumcraft.mac`, `~/Library/Logs/PlumCraft`, `~/.cache/plumcraft` (absent both times), `~/Library/Application Support/plumcraft-editor` (egui's real preferences), `~/Library/Preferences/com.plumcraft.mac.plist`, `~/.config/plumcraft`. Before and after are identical (`realcheck-before.txt`, `realcheck-after.txt`). The egui editor was never launched as an app; every picture is the kittest harness's.

## Pictures and what looks wrong in them

`claude-debug/egui-parity/shots/before/` and `after/`. B45: `b45-every-widget-count-hidden` (before: "amount" beside 0, "flag" beside 0.50, "mode" beside a check box, an empty pin at the bottom; after: each label beside its own field), `b45-replace-find-hidden`, `b45-opened-with-hidden-rows`, `b45-*-after-*`. Controls: `ctl-palette-mandel` (before "No matching nodes", after Mandelbrot "· starts the array engine"), `ctl-double-click-empty-canvas` (before nothing, after the palette there), `ctl-double-click-output-pin` (before the node selected and nothing else, after the pop-out), `ctl-popout-bar`, `ctl-parameters-show-on-face`, `ctl-settings-appearance`, `show-on-face-*`, `match-the-system-*`, `channel-g` (after only).

What looks wrong: the palette opened by a double click on empty canvas, with no query, showed only two kinds (Array to PDF and Array to SVG, both "starts the array engine") in that picture; the audit saw the same short list in a palette's first frames (it filled after a few more), and nothing here checked the full list, so a person should look in the real window. The Show on Face boxes have no column heading (their hover says "Show count on Face"). In the crate's own kittest pictures text fields are black on a light theme and the menu bar sits under in-editor windows; that is the harness without the app's fonts and with one viewport, as before. No "before" picture of the channel picker was kept (the old pop-out had none).

## What a person should try in the egui editor

1. Open a graph saved by the Apple apps with a row hidden (Show on Face off), or hide one in the parameters window: each label sits beside its own field; drag a number after the hidden row and watch only that port change.
2. With Engine › Pick Engines Automatically on and only the text engine running, press Tab, type "mandel", pick Mandelbrot: the array engine starts and the node appears where the palette was.
3. Double click empty canvas, a node, an output pin, a wire and a collapsed group; double click a frame's tab and a group's band.
4. Pop out a value and use Copy, Save As…, Share… (Finder should come forward with the file selected), Pin as A and Pin as B.
5. Settings › Appearance › Match the System, then switch the Mac between light and dark.
6. Pop out a picture (an image value) and choose R, G, B, A and RGB.

## Decisions for the maintainer

**Answered 2026-11-21** (asked as three questions, 1, 5 and 6 below): "1. Disable 2. Accept. 3. Close". So: Share is disabled in egui by the core (`platform.share`; egui declares no share sheet and no longer shows the file in the file manager); a new egui editor with no stored theme follows the system, accepted as is, nothing pinned; B46 is closed as not reproduced (`docs/BUGS.md`, "Closed, not reproduced"). Decisions 2, 3 and 4 were not asked. Record: note 24, "egui after N4".

1. Share in egui: keep "show the file in Finder or the file manager" as egui's answer to the `share` ask, or leave Share disabled in egui with a reason the core gives (which would need a shell capability the core reads, not built). **Answered: "Disable"** (built: `platform.share`).
2. The C ABI core holds a whole request while an engine starts; egui's document now holds just the add (`Document::hold_adds`). Move the C ABI core onto the document's hold too, so there is one mechanism (needs an xcframework rebuild and the Apple checks)?
3. The double-tap rule now uses the first tap's hit within 0.8 s: keep it for every shell (it changes the Apple apps at their next core build), or limit it to egui?
4. The channel picker covers decoded raster pictures only; also give arrays' view pictures a channel (egui would need their pixels on the CPU)?
5. A new egui editor with no stored theme now follows the system: an existing user who never chose a theme on a Mac in dark mode will see Dusk where they saw Paper. Acceptable, or store Paper for existing preference files that have no theme? **Answered: "Accept."**
6. Close B46 as not reproducible, or keep it open for a run on the Linux test host with the audit's exact sequence? **Answered: "Close."**
