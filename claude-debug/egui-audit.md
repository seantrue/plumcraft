# The egui editor at parity? An audit on the Linux test host

Commit audited: `057ed2d` (`origin/feat/native-n0`, "Merge pull request #134 from seantrue/native/n5-kit"), a fresh clone with its own `target/`, on the Linux test host (x86_64 Ubuntu 22.04, toolchain 1.100.0, no display, no audio device). Audit only: no product code changed, nothing committed or pushed. Date: 2026-11-21.

## The answer

**Mostly, but not fully, and not in the same shape.** On the floor (plan section 9) the egui editor is at least level with the Apple apps: 79 rows met against the Mac's 77, and ahead on three (custom editors, Compare's exact difference, the recipe display). Against what N3 and N4 added to the Apple apps it is at parity in the core-driven parts that slice 4 brought across (pushed faces, grown frames, routed backward wires, Run Selected, a dropped wire opening the palette, moves as undo steps, the dead rename branches gone), and it keeps its own older windows for most of the rest (parameters, settings, cache report, graph parameters, messages, attach, compare). It lacks several N4 behaviours the maintainer ruled on for every shell: the palette never offers the kinds of an engine not started; double clicks don't "open what they are on"; the Watched side column is still there; the pop-out bar has no Copy, Save As, Share, Compare pin or drag out; the Settings window can't choose Match the System; and egui's viewers keep their own state (sort, paging, audio, channel), so the core's viewer state, which scripts and the conformance suite check, can disagree with what egui shows. One real defect was found that a person would hit: **a row hidden from a node's face (`node.face`, Show on Face, which the Apple apps write into the workflow) shifts egui's face rows, and an edit on the face then changes the wrong port.** Its tests pass (396 of 396 in `plumcraft-ui` and `plumcraft-editor`, the conformance suite on egui included), but the suite passing on egui mostly proves the core under egui's dispatcher, not egui's drawing.

## How egui was exercised

- **No real window.** The host has no X or Wayland server and nothing to start one without installing: `Xephyr` needs a host display, `Xwayland` a compositor, and there is no `Xvfb`, `xvfb-run`, `weston`, `sway` or `cage`. The editor started with `--control 0` stops at once: "neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set" (`egui-audit/logs/launch-nodisplay.log`).
- **(a) The real editor in a kittest harness, rendered.** A scratch test module (`egui-audit/logs/audit.rs`, included from `apps/plumcraft-editor/src/lib.rs` by one temporary `#[path]` line in the audit clone only) builds the editor exactly as the app's own test helper does (`editor_with`: the text engine in-process, the Engine menu, the installer, preferences), in an `egui_kittest` harness with the wgpu renderer (lavapipe and the GPUs' Vulkan drivers are present), 1600 by 1000 points. Pictures are the harness's own render of the whole window (pop-outs are in-editor windows in a one-viewport harness).
- **(c) The control dispatcher.** Every call goes through `EditorApp::handle_control`, what the `--control` port serves, with the window drawn between requests (as the conformance suite's egui driver does), and each reply is logged (`egui-audit/<test>/replies.jsonl`).
- **Person-like input.** Pointer presses, double clicks, drags and keys sent to the harness at the positions the core locates (`geometry.locate`, mapped to the screen by the canvas's transform).
- Every run set `COMMONKIT_HASH=b3` and pointed `COMMONKIT_TEST_RUN`, `COMMONKIT_STORE_ROOT`, `COMMONKIT_MARK_DIR`, `PLUMCRAFT_CRAFT_ROOT`, `PLUMCRAFT_EDITOR_PREFS` (and, from the second batch, `TMPDIR`) at a scratch folder in the clone.

Pictures and replies: `claude-debug/egui-audit/a0-probe` to `a12-palette`; logs and scripts: `claude-debug/egui-audit/logs/`.

## The floor (plan section 9) walked against egui

"Core" means the item is the engine interface's or the session's, which egui uses in-process (`plumcraft-session`, `plumcraft-editor-core`), the same standard as slice 6's walk. Mac and iPad columns are slice 6's (`claude-debug/native-n4-slice6.md`). "Differs" marks a row where egui and the Apple apps differ in either direction. Test names are from this run's `plumcraft-ui` and `plumcraft-editor` suites (all passed); pictures are under `claude-debug/egui-audit/`.

| F | Item | egui | Evidence (egui) | Mac | iPad and iPhone | Differs |
|--|------|----|------------------------|---|---|---|
| F1 | Engine connection state | Met | Status bar "Text 0.7.0 · connected" (every picture); `kittest_engine_status` | Meets | Meets | |
| F2 | Reconnect with backoff | Met | `switching_text_engines_runs_after_every_switch` (`engine.reconnect`, then a run) passed | Meets | Meets | |
| F3 | Liveness / health | Met (core) | Shared crates; `crates/engine` tests not re-run here | Meets | Meets | |
| F4 | Catalog fetch | Met | Node › Add lists the catalogue (`a0-probe/menu.json`); `plumcraft-editor nodes --engine array,text` lists 114 lines | Meets | Meets | |
| F5 | Version gate | Met (core) | `crates/proto` tests not re-run here | Meets | Meets | |
| F6 | Engine identity, capabilities | Met | `menu.get` enabled states with reasons (Share… "save the graph first") | Meets | Meets | |
| F7 | Catalog hook | Met (core) | Core | Meets | Meets | |
| F8 | NodeDef core | Met | Faces from the catalogue (`a0-probe/01-start.png`) | Meets | Meets | |
| F9 | Wire types | Met | A person's wire drag is one step "Connect" (`a9-runs`); `kittest_convert`, `kittest_node_details` | Meets | Meets | |
| F10 | Families and icons | Met | Header icon and family colours (pictures); Settings › Node families | Meets | Meets | |
| F11 | Port widget fields | Partly | Every widget kind drawn on "Every widget" (`a2-inline/01-every-widget-node.png`); but a row hidden from the face misaligns the rest and edits the wrong port (defect D1) | Meets | Meets | Yes |
| F12 | Socket modes | Met | `kittest_widgets`, `kittest_params` | Meets | Meets | |
| F13 | Setting binding | Met | Parameters window sections (`a4-structure/02-parameters-inspector.png`); `inputs_are_grouped_with_the_spare_fields_last` | Meets | Meets | |
| F14 | Variadic inputs | Met | `canvas::tests::variadic_rows_grow_with_wires` | Meets | Meets | |
| F15 | Choice-dependent sub-ports | Met | `canvas::tests::choice_rows_follow_the_selection`, `the_canvas_tucks_what_the_choice_doesnt_use` | Meets | Meets | |
| F16 | Path hints | Met | `kittest_files` | Meets | Meets | |
| F17 | External ports | Met (core) | Core | Meets | Meets | |
| F18 | Machine-dependent choices | Met (core) | Core | Meets | Meets | |
| F19 | Node flags | Met | `a_node_menu_marks_a_markable_member_and_the_node_says_so` | Meets | Meets | |
| F20 | List typing | Met | Core; `the_sheet_is_a_labelled_grid_and_a_click_shows_that_item` | Meets | Meets | |
| F21 | Wire slices | Met | `a_wire_is_sliced_from_its_menu_labelled_and_cleared` (egui's slice field) | Meets | Meets | |
| F22 | Custom editors | Met | An application registers an editor kind; the port gets its button (`kittest_value_editors`, 10 tests); no JSON editor for a kind nobody registered | Partly | Partly | Yes (egui ahead) |
| F23 | Codegen metadata | Met (core) | Core | Meets | Meets | |
| F24 | Expansions | Met | `kittest_expand` (4) | Meets | Meets | |
| F25 | Style graphs | Met (core) | Core | Meets | Meets | |
| F26 | Run request | Met | Run menu, ⌘Return; runs in every flow | Meets | Meets | |
| F27 | Targets | Met | Run › Run Selected ran only the selected node and what it needs (`a1-palette`: 2 of 3 nodes) | Meets | Meets | |
| F28 | Run identity, generation | Met (core) | Core | Meets | Meets | |
| F29 | Latest wins | Met (core) | Core | Meets | Meets | |
| F30 | Auto-run | Met | Run › Auto-Run; status bar | Meets | Meets | |
| F31 | Optimistic queued | Met | `a_run_shows_each_node_waiting_running_then_done`; `a9-runs/02-running-wash.png` | Meets | Meets | |
| F32 | Run reply | Met (core) | `ui.waitRun` replies | Meets | Meets | |
| F33 | Per-node report | Met | Face status and timing (pictures) | Meets | Meets | |
| F34 | Errors as data | Met | Fail node's face, "1 error" chip, Messages window (`a6-windows/01`, `02`) | Meets | Meets | |
| F35 | Failure stands in for a reply | Met (core) | Core | Meets | Meets | |
| F36 | Cancel | Met | Cancelled four ways (command, `ui.invoke`, ⌘Period, the status bar's Cancel), the slow node `cancelled` within 2.4 s (`a10-cancel`); one earlier cancel not honoured, not reproduced (D9) | Meets | Meets | |
| F37 | Run summary for automation | Met | `ui.waitRun`, `ui.inspect` run object | Meets | Meets | |
| F38 | Status record stream | Met (core) | Core | Meets | Meets | |
| F39 | States emitted | Met | `kittest_run_anim` (4) | Meets | Meets | |
| F40 | Reducer rules | Met (core) | Core | Meets | Meets | |
| F41 | Canvas status display | Met | Header status text (pictures) | Meets | Meets | |
| F42 | Per-node timing | Met | "0.30 ms" on faces | Meets | Meets | |
| F43 | Parameter wires, bundles | Met | `kittest_bundles` (8) | Meets | Meets | |
| F44 | Provenance | Met (core) | Core | Meets | Meets | |
| F45 | Stale tracking | Met | `kittest_run_status` stale tests (4) | Meets | Meets | |
| F46 | Text on nodes | Met | Excerpts on faces ("PLUM") | Meets | Meets | |
| F47 | Thumbnails in flight | Met | Picture and waveform thumbs on faces (`a5-viewers/16-audio-playing.png`) | Meets | Meets | |
| F48 | Texture cache | Met (read in code) | Textures keyed by handle; not measured | Meets | Meets | |
| F49 | Inspect piggybacks on runs | Met | A click on a title watches and runs (`clicking_a_title_watches_and_runs`) | Meets | Meets | |
| F50 | Display payload | Met | Every viewer kind popped out (`a5-viewers`) | Meets | Meets | |
| F51 | View modes | Met | Viewer tabs on every pop-out (Table/Tree, YAML/Tree, Audio/Bytes, Picture/Bytes) | Meets | Meets | |
| F52 | Pixel probe | Met | `a_click_pins_the_probe_and_the_command_does_the_same`, `an_images_probe_reads_its_own_16_bits` | Meets | Meets | |
| F53 | Compare | Met | Side, split and exact difference (`the_difference_of_two_known_arrays_is_exact`; `a6-windows/07-compare-split.png`) | Partly | Partly | Yes (egui ahead) |
| F54 | Tables and scalars | Met | egui's table: header sort (local), all rows virtualised, colour by value (`a5-viewers/04`) | Meets | Meets | |
| F55 | Sheet | Met | `the_sheet_is_a_labelled_grid_and_a_click_shows_that_item` | Meets | Meets | |
| F56 | Recipe display | Met | `kittest_recipe` (5): the panel says what made the value | Does not | Does not | Yes (egui ahead) |
| F57 | Picture window (T16) | Met | Pop-out windows (in-editor here; native viewports with a display, not seen) | Meets | Meets | |
| F58 | Missing data is a state | Met | `a_value_that_isnt_here_says_so_and_shows_no_number`; "no value" | Meets | Meets | |
| F59 | Curve editor | Not met | The research software's (N6); egui has the hook (F22), not the editor | Does not | Does not | |
| F60 | Mask editor | Not met | As F59 | Does not | Does not | |
| F61 | List mapping with axes | Met (core) | Core | Meets | Meets | |
| F62 | Vary nodes | Met (core) | Node › Add › Vary (menu) | Meets | Meets | |
| F63 | Lineage | Met (core) | Core | Meets | Meets | |
| F64 | Save picture with recipe | Met | `a_result_nodes_button_asks_where_and_the_engine_writes_the_file`, `the_watched_panel_saves_the_item_shown_and_only_what_a_format_takes` | Meets | Meets | |
| F65 | Export group as plugin | Not met | The research software's pack | Does not | Does not | |
| F66 | Deliver plugin | Not met | As F65 | Does not | Does not | |
| F67 | Plugin round trip | Met (core) | Core expansions | Meets | Meets | |
| F68 | Arrays never in messages | Met (core) | Core | Meets | Meets | |
| F69 | Content addressing | Met | "hash b3 · environment" in the status bar | Meets | Meets | |
| F70 | Hostile `.npy` and keys | Met (core) | Hostile sweeps not re-run here | Meets | Meets | |
| F71 | Engine memo | Met | Cache Report (`a6-windows/03-cache-report.png`); "cached" faces | Meets | Meets | |
| F72 | Partial re-run | Met | "cached 2 of 2" in the status bar | Meets | Meets | |
| F73 | Preview latency path | Met (core) | In-process engines | Meets | Meets | |
| F74 | Full resolution on demand | Met | `scrolling_in_past_the_picture_asks_for_detail_once` | Meets | Meets | |
| F75 | Pictures as engine-side files | Met (core) | Core | Meets | Meets | |
| F76 | Workflow JSON is the payload | Met | Saved, reopened (`a7-settings`) | Meets | Meets | |
| F77 | Recipes in pictures | Met (core) | Core | Meets | Meets | |
| F78 | Bundles | Met | `graph.bundle` wrote the zip (`a9-runs`); `a_bundle_goes_out_through_the_file_menu_and_comes_back_where_its_paths_work` | Meets | Not walked | Yes (iPad not walked) |
| F79 | Insert graph or group | Met | `graph.insert` and `graph.insertGroup` (`a9-runs/04-after-insert.png`) | Meets | Meets | |
| F80 | Engine-touching commands | Met | Enabled states with reasons; Attach… window (`a6-windows/09-attach.png`); Install Engine… (`kittest_install`) | Meets | Meets | |
| F81 | Control channel | Met | The dispatcher drove this whole audit; `crates/ui/tests/control.rs` passed; the TCP port itself not opened (no display) | Meets | Meets | |
| F82 | Headless CLI catalog | Met | `plumcraft-editor nodes` | Meets | Meets | |
| F83 | Config by environment | Met | `COMMONKIT_HASH` read ("hash b3 · environment") | Meets | Meets | |
| F84 | Single-threaded engines | Met (core) | Core | Meets | Meets | |
| F85 | Local model management | Not met | The research software's CLI | Does not | Does not | |

Summary for egui: 79 met (24 of them through the core alone, and 3 of those resting on tests not re-run here: F3, F5, F70; F48 read in code), 1 partly (F11, defect D1), 5 not met (F59, F60, F65, F66, F85, the same as the Apple apps). Against the Mac (77 met): egui is ahead on F22, F53 and F56, behind on F11. Against the iPad and iPhone (76 met): the same, plus F78 not walked there.

## N3 and N4 features in egui

"Has" means seen working in egui here; "Older" means egui has its own earlier way of doing the same; "Lacks" and "Broken" are gaps; "Native only" is by design.

| Feature (stage, slice) | egui | Evidence and difference |
|--------|---|------------------------|
| Moves as undo steps (N3 s1) | Has | Dragging a pushed face is one "Move Node" (`a3-routes`); `the_arrow_keys_nudge_the_selection_and_a_held_key_is_one_step` |
| A new wire dropped on nothing opens the palette for what takes it (N3 s1) | Has | Scene `view.palette` with the wire; egui draws the palette there (`a8-followups/02`). The dragged wire is not left drawn while the palette is open (the Apple apps keep it) |
| Palette offering kinds of engines not started (N4 s1) | Lacks | `palette.items {query: "mandel"}` gives 0 with picking off and with it on; Tab and typing says "No matching nodes" (`a1-palette/03`). egui never fills `Engines::offered` or `offered_defs` (only `crates/ffi/src/core.rs` does, read in code). `palette.choose` of such a kind starts the engine but refuses: "it is starting: add the node again once it has connected" |
| Palette on a double click on empty canvas (N3 decision 2) | Lacks | A double click did nothing (`a1-palette/01`); egui opens it by Tab, Space or a right click |
| Run Selected as a core command (N4 s1) | Has | Run menu, enabled with the core's reason; ran only the selection (`a1-palette`) |
| Inline editing on the face (N4 s2) | Older | egui's own widgets are always live on the face (text fields, drag values, check boxes, combo boxes, colour); the core's field editor (`row.edit`, scene `view.edit`) is ignored: a script that opens one sees nothing (`a2-inline/02`); `row.set` works and is one step "Set Value" |
| Scrub (N4 s2) | Older | egui's drag values scrub; the core's scrub gesture can't reach egui's rows, which egui doesn't report ("face n3 has no widget measured on row 1") |
| The far-zoom floating editor (N4 s2) | Lacks (by design) | egui's widgets scale with the canvas |
| Faces never overlap: the push (N4 s2, egui in s4) | Has | Graph keeps (0, 20), the scene and egui show (0, 96) (`a3-routes/01`) |
| Frames grown round a pushed member (N4 s3, egui in s4) | Has | `a3-routes/01`; `a_frame_round_a_pushed_member_is_drawn_grown` |
| Routed backward wires, drawn and hit as routed (N4 s2, egui in s4) | Has | Drawn along the core's route (`a3-routes/02`); a click at the core's middle watches that wire's value. Forward wires agree too (`a9-runs`) |
| Icons and the more-button (N4 s2) | Has (own) | Header icon and ⋮ drawn by egui; the core's `geometry.locate {part: "menu"}` says "no menu measured", since egui doesn't report header parts |
| Parameters inspector (N4 s3) | Older | egui's own window, richer (filter, sliders, segmented choices, help); no Show on Face control |
| Show on Face (`node.face`, N4 s3) | Broken | No UI in egui, and a hidden row breaks the face (D1) |
| Double tap opens what it is on (N4 s3, card 3 at N3: "the same in every shell") | Lacks | A double click on a face's header only watches (`a4-structure/01`); on an output pin only watches (`a8-followups/03`); on a frame's title egui opens its rename window (`a_double_click_on_the_title_opens_the_rename_window`) |
| Groups, the way back out (N4 s3) | Has (own) | Breadcrumb "Graph › group 1 · Leave group" (`a4-structure/06`) |
| Names edited in place (N4 s3) | Older | egui shows a small window for the core's name editor (group, frame, net, port), titled e.g. "Rename group group 1" (`a4-structure/06`) |
| Nets with the driver's tag and count (N4 s3) | Has | `kittest_net_labels` (4); my scripted net steps failed on my own wrong call, not walked by hand |
| Pop-outs replacing the Watched column ("Eliminate the side column in favor of just the popout window") | Not applied | egui keeps the Watched column; pop-outs come from its "Pop out" button. A double click on a pin doesn't pop out (the Apple apps' rule, contract 12.20) |
| Pop-out windows as the core's (`view.windows`, `popout.get`) | Has | `view.popout` opens egui's window; scene and `popout.get` agree |
| The settings model (N4 s4) | Older | egui draws its own Settings window (Appearance, Engines, Cache) and serves `settings.get` |
| Themes following the system (N4 s4) | Partly | `view.theme {name: "system"}` works and egui follows `ctx.system_theme()` (read in code); the Settings window shows only Paper, Pewter and Dusk, no Match the System (`a7-settings/01`), though `settings.get` lists it; a new egui editor starts on Paper, the C ABI core on system |
| Increase Contrast, Reduce Transparency, Reduce Motion (N4 s4) | Lacks (read only) | Shown as notes ("Off") in `settings.get`; egui has no `lifecycle.appearance` |
| Autosave in the core (N4 s4) | Has (core, not used) | `document.autosave` saved after 1 s; title and dirty followed (`a7-settings`). Neither egui nor the Mac turns it on (by design, iPad only) |
| Duplicate, Rename, Save a Copy, Move (N4 s4) | Has (commands) | `graph.duplicate`, `graph.rename`, `graph.saveCopy` worked; no egui menu items (the iPad's title menu is native only) |
| Number text from the core (N4 s4) | Has | Conformance case "a number shows the same text everywhere" passes on egui; faces show "0.50", "2.0" |
| The Messages window (N4 s5) | Older | egui's own (`a6-windows/02`), `messages.get` served; `kittest_status` (6) |
| Viewers as the core's data (N4 s5) | Older | egui's viewers keep their own state: see the rows below and D4 |
| Table sort and paging (N4 s5) | Older | A header click sorts locally; `popout.get` still says unsorted (`a5-viewers/04`); `viewer.table {page: 1}` changes nothing shown (all 1,200 rows virtualised) |
| JSON as YAML with an outline ("format as Yaml for display") | Has | YAML first in the Watched panel and pop-out, Tree beside it with Raw JSON (`a5-viewers/06`); the outline is egui's, not `viewer.tree`'s |
| Audio waveform and spectrogram (N4 s5) | Has (own) | Both drawn (`a5-viewers/16`); B36's spectrogram on its own thread. `viewer.audio {play: true}` set the core's state to playing while egui's player stayed stopped |
| Picture channels (N4 s5) | Lacks | No channel picker (Fit, 1:1, −, +); `viewer.picture {channel: "r"}` accepted and not drawn; the readout names every channel (`every_channel_reads_under_its_own_name`) |
| Picture detail on zoom (N4 s5) | Has | `scrolling_in_past_the_picture_asks_for_detail_once` |
| Array grid paged by the core (N4 s5) | Older | egui's `array.grid` with formats and colour by value; `viewer.grid` paging not followed |
| SVG drawn, source a viewer away (N4 s5) | Has | Viewers svg, text.plain, hex (`a5-viewers/14`) |
| Array to Audio, Audio to Array (N4 s5) | Has | Both ran; Audio to Array's value is "array 49152×1 <f4" |
| Waveform thumbnails on faces (N4 s5) | Has | `a5-viewers/16` |
| Cache Report (N4 s6) | Older | egui's own window (`a6-windows/03`); `report.get` served |
| The graph's Parameters window (N4 s6) | Older | egui's own (`a6-windows/04`): Go to, Rename…, Remove, reorder |
| Value editors (N4 s6) | Older, ahead | egui draws an application's registered editor kinds; the Apple apps draw any port's value as JSON. A node with no editor: "names no editor" |
| Compare and pinning from a pop-out (N4 s6) | Partly | Compare is in the Watched panel (Pin A, Pin B, A│B, Split, │A−B│); `viewer.pin {slot, popout}` works by command, but no pop-out offers it |
| View As, Copy, Share…, Save As… on every viewer's bar (N4 s5) | Partly | Viewer tabs on every pop-out; Copy (YAML) and save in the Watched panel; the pop-out bar has none of Copy, Share, Save As (`a6-windows/09`) |
| Drag out of a viewer (N4 s6) | Lacks | No drag handle; `value.export` works by command (and see D2) |
| Attach… through the form ask (N4 s6) | Older | egui's own Attach window (`a6-windows/09`) |
| Install Engine… (N4 s6) | Has, ahead | egui installs (`kittest_install`); the C ABI core refuses |
| Text size (N4 s6) | Has, ahead | `view.textSize` 11 to 18 (`a8-followups/08`); removed from the Apple apps |
| Watching a value | Has | A click on a title, a pin or a wire watches into the Watched column |
| Select mode, Share… as core commands (N4 s3) | Has | View › Select Mode, File › Share… (disabled until saved; invoked on a saved graph it raised no ask that egui showed) |
| The core's toolbar model (N4 s3) | Lacks | `menu.get {context: toolbar}` is served; egui draws no toolbar (menus and status bar) |
| Every viewer's state a command (N4 s5) | Lacks in drawing | egui reads none of `viewer.table`, `viewer.tree`, `viewer.audio`, `viewer.picture`, `viewer.grid`, `viewer.hex` (none appears in `crates/ui/src`, read in code) |
| Sheets, iPhone compact layout, platform pickers, share sheets | Native only | By design |

## Regressions and defects found

1. **D1 (high, egui only): a row hidden from a face shifts the face's rows, and edits go to the wrong port.** Reproduce: add "Every widget" (`text.widgets`), then `node.face {node, port: "count", shown: false}` (what Show on Face writes into the workflow in the Apple apps, so a graph saved there and opened in egui shows it). egui's face then shows "amount" beside a field holding 0, "flag" beside 0.50, "mode" beside a check box, "tint" beside a multi-line box holding "#3366cc", and an extra empty pin at the bottom; it stays so after 30 frames. A drag on the number field beside "amount" changed `count` from 3 to 4 and left `amount` at 0.5 (`a8-followups/06`, `07`, `replies.jsonl`). The same with `find` hidden on Replace: "with" is drawn beside find's field (`a9-runs/05`). Likely cause: egui builds its rows from the node definition but its labels and pins from the scene's shown rows, or the other way.
2. **D2 (medium, in the core, seen through egui): `value.export` of a short text value writes an empty file.** Reproduce: Text "forward" into Upper case, run, `view.popout` Upper case's text, `value.export`: `bytes: 0`, the file empty, while `popout.get` has `value: null` and the excerpt "FORWARD" (`a9-runs`, `a6-windows`). `crates/editor-core/src/export.rs` `file_bytes` takes `Media::Text` with no full value from `d.value` and writes "" when it is absent. A table exported correctly (21 bytes). The same code serves the Apple apps' drag out; not checked there.
3. **D3 (medium, egui): the palette never lists an unstarted engine's kinds, and choosing one refuses.** See the feature table. Cause (read in code): `Engines::offered` and `offered_defs` are set only in `crates/ffi/src/core.rs`.
4. **D4 (medium, egui): egui's viewers and the core's viewer state disagree.** A person's header click sorts the table while `popout.get` says unsorted; `viewer.table` sort and page, `viewer.audio` play, `viewer.picture` channel, `viewer.grid` page and `viewer.tree` are accepted and change nothing on screen (`a5-viewers`). The conformance viewer cases pass on egui because they read the core's state, not egui's drawing.
5. **D5 (low, egui): the core's field editor and measured parts are invisible to egui.** `row.edit` opens a core editor egui never shows; egui reports no rows or header parts, so `geometry.locate {row}` and `{part: "menu"}` fail and 14 conformance steps (`only: rows`) never run on egui.
6. **D6 (low, egui): no Match the System in the Settings window,** and a new egui editor starts on Paper while the C ABI core starts on system.
7. **D7 (low, egui): double clicks don't follow the core's rule** (empty canvas: palette; face: parameters; pin: pop out).
8. **D8 (cosmetic, egui): the rename window's title repeats the word** ("Rename group group 1").
9. **D9 (unconfirmed): one Cancel Run not honoured.** In the first run of `a9-runs`, `ui.invoke run.cancel` during a 5 s Slow node returned `invoked`, and the node finished `done` at 5,008 ms. Seven later cancels (`a10-cancel`, `a11-cancel`, the same sequence uncached) all cancelled within 2.4 s. The first run's replies were overwritten by a rerun; recorded here only.

Checked and **not** found broken: the dead rename branches are gone (no `(None, "rename.…")` in `crates/ui/src`); egui draws backward and forward wires where the core hit-tests them; undo and redo across groups, frames, names and nets went back 15 steps and forward 15 (`a4-structure`); save, reopen, dirty and the title followed (`a7-settings`); the palette's list shows in full once drawn a few frames (an early short list was the harness's first frames, `a12-palette`).

## Where egui is ahead of the Apple apps

- **Compare's exact difference** (│A−B│, with tests on exact arrays); the Apple apps show side by side and split only.
- **The recipe display** (F56): a panel saying what made a value; the Apple apps have none.
- **Application value editors** (F22): an application's own editor kind drawn natively; the Apple apps show JSON.
- **Text size** (11 to 18) and **Zoom** in Settings; theme colour tokens, fruits, pinned families, theme import and export.
- **Install Engine…** works (the C ABI core refuses it).
- **Viewers:** table colour by value; text Find and line numbers; JSON Raw toggle; array histograms, view picker, plots (`array.plot`), grid formats and colour by value; PDF, doc, XML and mesh viewers; picture −/+ buttons and a checkerboard behind alpha.
- **The parameters window:** a filter, sliders, segmented choices, help per field.
- **Bundles:** badges on bundled values and detach (`kittest_bundles`).
- **Save from the Watched panel** with a contact sheet for lists (`kittest_save`).

## Test health for egui on Linux at `057ed2d`

| Suite | Result |
|--------|------------|
| `cargo nextest run --all-features -p plumcraft-ui -p plumcraft-editor` | 396 run, 396 passed, 7 skipped (ignored: picture writers and network installs), 30.5 s |
| of which the conformance suite on egui (`crates/ui/tests/conformance.rs`) | `the_suite_passes_on_the_egui_editor` and `the_array_cases_pass_on_the_egui_editor` passed: 52 cases, 5 of them on the array engine; 23 of the suite's 720 steps never run on egui (`only`: rows 14, media 5, reports 2, prefs 1, port 1) |
| `cargo nextest run --all-features -p plumcraft-editor-core -p plumcraft-conform` | 188 run, 187 passed, 1 failed, 2 skipped |
| the failure, `prefs::tests::each_application_has_its_own_file` | Caused by this audit's environment: it asserts the per-application path while `PLUMCRAFT_EDITOR_PREFS` pointed at scratch; run again without that variable it passed |
| The audit's own scratch tests (`egui_audit::a0` to `a12`) | 13 passed (they record, they don't assert) |

Known intermittents B37 to B40 and the known `params_coverage` failure are outside these suites and did not appear.

## What could not be audited here

- **A real window.** No display server and none startable without installing (above). Not seen: native viewports for pop-outs (each its own OS window), window focus and ordering (B19), OS file dialogs (rfd), the system clipboard, drag and drop, the system's dark mode or contrast changing while running, the `--control` TCP port and token file (needs the window), real input timing, frame rate.
- **Audio playback.** No audio device; the player was given a silent output. Waveform and spectrogram were drawn; nothing was heard.
- **The craft apps.** Open in VectorCraft and the other document apps were not started; the vectorcraft kittests passed but may skip without their fixtures (not checked).
- **The Apple apps themselves.** Their columns are slice 6's report, not rerun.
- **Performance and budgets** were not measured.
- **Mac and Windows look** of egui (fonts, menus) were not seen; only the Linux render.

## Gaps ranked, with a rough size for closing each

| Rank | Gap | Size | Note |
|-|------------|---|------------|
| 1 | D1: faces with a hidden row misdrawn and edited wrongly | Small to medium | A person editing a graph saved by the Apple apps changes the wrong value; fix before egui is used on such graphs |
| 2 | D2: empty export of a short text (core) | Small | Use the excerpt or fetch the value; affects drag out everywhere |
| 3 | The pop-out bar: Copy, Save As…, Share/Export, Pin as A/B, a drag handle | Small | The commands exist (`value.copy`, `value.saveAs`, `value.export`, `viewer.pin {popout}`); egui only lacks the buttons |
| 4 | D3: the palette's unstarted engines | Small | Fill `offered` and `offered_defs` from the editor's choices as the C ABI core does, and hold the choose while the engine starts |
| 5 | D7: double clicks by the core's rule | Small | Empty canvas: palette; face: parameters; pin: pop out |
| 6 | Show on Face in egui's parameters window | Small | After D1 |
| 7 | D6: Match the System in Settings, system as the default | Small | |
| 8 | Picture channel picker | Small | `viewer.picture` exists |
| 9 | D4: egui's viewers following the core's viewer state (table sort and paging through `viewer.data`, audio, tree, grid) | Medium to large | Or a decision that egui keeps local viewer state, with the conformance cases marked accordingly |
| 10 | The Watched side column removed in favour of pop-outs | Medium | A decision first: egui's Compare, recipe and save live in the column today and would move to the pop-out |
| 11 | D5: egui reporting rows and header parts to the core | Medium | Lets the 14 `only: rows` steps run on egui and `ui.gesture` reach its rows |
| 12 | The core's toolbar | Small to medium | Optional; egui has menus |
| 13 | Names edited in place rather than in a small window | Medium | Optional; egui's windows work |
| 14 | D8: the rename window's title | Small | |
