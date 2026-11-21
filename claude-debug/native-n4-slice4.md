# N4 slice 4: settings and themes, autosave in the core, the network port for debug builds, egui catching up

Branch `native/n4-settings` (from `feat/native-n0` with N3 and N4 slices 1 to 3), 2026-11-20. Uncommitted, for the controller to review.

## First: what this slice did that it should not have

**My gate run wrote into the maintainer's real store.** The gate script I wrote (`claude-debug/native-n4/s4-gate.sh`) set the scratch environment for cargo but not for the Python stage. Python ran between 20:57:12 and 20:58:05 with the login shell's `COMMONKIT_STORE_ROOT=/Volumes/<store>/store`. The suite's own guard stopped one test (`test_hash_is_what_was_asked`: "COMMONKIT_TEST_RUN is unset"), but others wrote:

- eight handshake nonces in `.hello/` (`e-22174-*` five, `e-22404-*`, `e-22416-*`, `e-22450-*`), 64 bytes each, from test processes that have exited;
- one new folder `plummemo/ea` holding one memo entry, `ead979a3…bfb.c.npy` (258 bytes).

I did not delete them: removing things from the maintainer's store is the maintainer's call. They are listed in `claude-debug/native-n4/s4-store-written-by-mistake.txt`. Nothing else in the two-level listing changed. The Python suite was then rerun under the scratch environment (224 passed), and the script now sources `s3-env.sh` for that stage.

## On the maintainer's screen

- Nothing modal appeared, and nothing of mine took the keyboard or activated an app.
- The Mac app launches (`conformance.sh` twice, the picture driver twice, the Release refusal proof once) were all scripted (`--scratch-state`), never activated. Their windows were ordered just above their own document window: the Settings window and the About panel included, as the pop-outs were in slice 3. They were on screen behind the maintainer's windows while they ran.
- The maintainer's apps, PIDs 5861, 54729, 56625, 58303 and 60555, were never signalled. Nothing was built into `native/apple/PlumCraftMac/build`, and `claude-debug/native-n3/s6-try/` was not touched. Mac builds went to `claude-debug/native-n4/s4-build/mac`.

## The device

- **Installed:** this branch's Debug build of `com.plumcraft.pad` at 21:00:28. It replaced the one there, signed automatically with the existing team identity and profile, with no `-allowProvisioningUpdates`.
- **Launched:** one launch was tried at 21:00:39. It was refused: "Unable to launch com.plumcraft.pad because the device was not, or could not be, unlocked."
- **What that means:** the iPad was locked, so per the brief device work stopped there. No app ran on it, no port was opened, and nothing else on the device was touched.
- **State now:** the debug build is installed and not running. It listens on the network only when launched with `--control-bind`.
- **Not run:** the suite on the device, the UI tests on the device and the budget measurement.
- **The recipe:** `claude-debug/native-n4/s4-ipad-device.sh [conformance|ui|budget|all]` takes them all in one command once the iPad is unlocked. It installs this branch's Debug build and launches it with `--control-bind lan --control-port 47811 --control-token <fresh>`. It reads the app's address back from its status file (`devicectl device copy from`), runs the whole suite from the Mac with `plumcraft-conformance --host`, then quits the app by its PID. It then runs the UI tests and `claude-debug/native-n3/s6-ipad.sh`, which stops and says so at 60 Hz.
- **A permission prompt:** iOS may ask once whether the debug build may use the local network (the build's Info.plist gives its reason). A person has to answer that on the iPad.

## What was done, item by item

| Item | State | Notes |
|---|---|---|
| 3. Autosave in the core | Done | Below; the iPad uses it, the Mac keeps Save |
| 4. Network control port, debug builds | Done | Guarded in the core and the shell; a Release build refuses it (shown) |
| 5. The suite on the device | Not run | Installed; the launch was refused because the iPad was locked; one-command recipe left |
| 1. Settings | Done | A core model; native on both apps; About on both; off the banner |
| 2. Themes | Done | Match the System by default; contrast and transparency in the tokens; Reduce Motion in the canvas |
| A. "No watcher popout at all" | Fixed in the core | A watch pops the value out where there is no Watched panel |
| B. Sliders and precision | Fixed in the core and the kit | Live field, one step per drag, the core's number text |
| 6. egui catches up | Done (subagent) | Pushed faces, grown frames, routed backward wires; dead rename branches removed |
| 7. Conformance | Done | 41 cases (5 new), every driver, no skips |

## Settings, and where each lives

Every setting is in the core's model (`settings.get`, contract 14.4) and is changed by the command its item names. Both apps render it with one SwiftUI form in the kit (`PlumCraftShell/SettingsView.swift`):

- **Mac:** a Settings window, one tab per section, opened from the app menu's Settings… (⌘,).
- **iPad:** an in-app sheet listing the sections, each a page, opened from the app menu's Settings… and a gear in the bar. No Settings app bundle: nothing needs setting before launch.

The window's open state is the core's (`view.windows.settings`), so `app.settings {tab}` from a script opens it exactly as a person does.

| Setting (egui window) | Command | Kept in |
|---|---|---|
| Theme (Match the System, Paper, Pewter, Dusk) | `view.theme {name}` | prefs `theme` |
| Text size (11 to 18 pt) | `view.textSize {size}` | prefs `appearance.text_size` |
| Zoom (0.8 to 1.6) | `view.zoom {zoom}` | prefs `appearance.zoom`; offered by egui only |
| Each colour of the theme shown (30 tokens, 6 groups), its contrast badge, reset | `theme.setToken {token, theme, color}` | prefs `appearance.overrides` |
| The 14 fruits | `theme.setFruit {fruit, theme, color}` | prefs `appearance.fruits` |
| Each catalogue family pinned to a fruit, or its own | `theme.pinFamily {family, to}` | prefs `appearance.families` |
| Import Theme…, Export Theme… | `ui.invoke theme.import / theme.export` (a file ask) | the file; the import into prefs |
| Reset the theme's colours | `theme.reset {theme}` | prefs |
| Colour Nodes While Running (View menu in egui) | `view.runColors {on}` | prefs `run_colors` |
| Animation Speed (View menu in egui) | `view.animationSpeed {ms}` | prefs `anim_ms` |
| Pick Engines Automatically | `engine.auto {on}` | prefs `auto_engines` |
| Open Cache Report | `run.cacheReport {open}` | the document (still unbuilt in the Apple apps: banner) |
| Increase Contrast, Reduce Transparency, Reduce Motion | read only, from the system (`lifecycle.appearance`) | not kept |

**Where the preferences file is:**

- **The C ABI's core:** `dirs.prefs/prefs.json`, with `PLUMCRAFT_EDITOR_PREFS` winning.
- **The Mac app:** `~/Library/Application Support/PlumCraft/prefs.json`, its own file, separate from the egui editor's `plumcraft-editor` (a decision below).
- **The iPad:** its container. Every script points `PLUMCRAFT_EDITOR_PREFS` at scratch, and a scripted Mac launch now refuses to start without that set to scratch.

**About:** the standard About panel on the Mac and a small sheet on the iPad. Both show `app.about`'s title and version (from the config's `app`) and its parts: Editor core 0.7.0, Shell interface 1, C ABI 1, and the engine.

**Not honoured in the Apple apps (reported):**

- Zoom is not offered: the system scales the display, and the canvas zooms itself.
- Text size changes the core's text sizes but not yet the Apple canvas's fonts, which follow the system size.

## Themes

`view.theme {name: "system"}` is the default for a native core: Paper in light mode, Dusk in dark. Both canvases now send `lifecycle.appearance {dark, contrast, reduceMotion, reduceTransparency}` when the effective appearance or the accessibility display options change, instead of naming a theme. A theme chosen in Settings stays whatever the system does, and the window chrome still follows the system: the Dusk canvas in a light window is in `mac-canvas-theme-dusk-light.png`.

Accessibility:

- **Increase Contrast:** the core moves dim and faint text, separators, field borders, wires, the grid and default pins towards the text colour. A test checks that no contrast pair falls, on every theme.
- **Reduce Transparency:** every token is made opaque over the canvas.
- **Reduce Motion:** the canvas holds a running face's wash instead of pulsing it.

What tokens can't reach: the system materials (the Mac's path control vibrancy) and the Settings and About views, which are the system's own controls and follow the system's settings themselves.

## Autosave

How it works (`crates/editor-core/src/autosave.rs`, contract 4.4):

- **When it saves:** once a change has settled (1 s with no other), or at most 10 s after the first unsaved change while changes keep coming.
- **What it waits for:** never while a run is in flight; the change waits for the run to end. A graph with no file is `untitled` and isn't saved.
- **What counts as a change:** a new `Session::changes` count rises with everything that dirties the graph, including Show on Face (`node.face`), which is not an undo step and is now saved.
- **How it writes:** the graph is serialised on the core's thread and written on a thread of its own, atomically (temporary file, sync, rename), one save at a time.
- **Dirty:** a save of change *n* cleans the document only if nothing changed while it was written. `dirty` means "not yet saved".

**Failure:** the state is `failed` with the reason, the document stays dirty, and the save is tried again after 5 s (doubling to a minute) or as soon as the next change settles. The iPad shows "Couldn't Save" in the title's subtitle and says the reason once in its banner, with no dialog (`pad-autosave-failed-*.png`). Only closing a document whose save fails asks: Close Without Saving, or Keep Open.

**Flush:** `lifecycle.background`, `document.close` and closing the core save at once, waiting up to 10 s for a save in flight. The iPad calls `lifecycle.background` from `sceneDidEnterBackground` inside a background task, after ending text editing.

**The feed:** the document domain carries `autosave: {on, state, error?, saves, failures}`.

**iPad:**

- Every graph opens with `autosave: true`, and there is no Save button.
- The title's subtitle says Saving… while a change waits or is written.
- The title menu has Duplicate (`graph.duplicate`), Rename (the system's rename in the title, `graph.rename`), Save a Copy… (`graph.saveCopy`, the `file.save` ask), Move… (`graph.move`, the `folder.pick` ask) and Share….
- A memory warning reaches the core as `lifecycle.memory {level: "low"}`, which drops the media caches.

**Untitled documents** live where the document browser's + makes them: `Untitled.json` in the folder it shows (On My iPad › PlumCraft by default), named uniquely by the system.

**Mac:** unchanged, with NSDocument's Save and the edited dot.

## The network control port

Core config: `control: {port, bind: "lan" | address, token, network_debug}`.

- **Refused by the core** unless `network_debug: true` and a pre-shared token (`token`, 64 hex, never printed, or `token_file`) are both given.
- **`lan`** is the address the device would send from, and only if it is private, link-local or unique-local. A named address must be on the local network; `0.0.0.0` is refused.
- **Peers** not on the local network are closed before anything is read, and every connection still authenticates first. The existing token rule, framing and limits are unchanged.
- **The Apple apps:** `ShellConfig.controlBind` and `controlToken` exist only under `#if DEBUG`. In a Release build, `--control-bind` logs a refusal and the port stays on loopback.
- **The iPad's Info.plist:** debug builds get `NSLocalNetworkUsageDescription` (merged per configuration); Release builds don't.

**The proof** (`claude-debug/native-n4/s4-release-refuses-lan.txt`): the Release Mac app, launched with `--control-bind lan --control-port 47812 --control-token …`, printed "PlumCraft: --control-bind lan refused: a release build keeps the control port on loopback", and `lsof` showed it listening on `127.0.0.1:50378` only. It was quit by its PID.

**Rust tests:** without the flag, without a token, `0.0.0.0`, `8.8.8.8`, a bad token and a wrong type are each refused. On `::1` with the flag and a token, a wrong token is refused and the right one is served.

Nothing of mine is listening on the network now. No Mac app of mine ever bound beyond loopback.

## The maintainer's two defects

**A. "No watcher popout at all."**

- **The cause, from the code:** a click on a pin is a `ui.gesture` tap, which the core turned into `run.watch` alone, and every Watch item in the menus is `run.watch` too. With the Watched column gone, nothing showed the value; only a double click or Pop Out opened a window.
- **The fix, in the core:** a document whose shell has no Watched panel (`Document::watch_pops_out`, set for every C ABI document) pops a watched port's value out, bringing an open one forward. `run.watch {node}` (a title, a preview) still only watches.
- **Two Mac guards:** a remembered pop-out frame mostly off every screen is centred, and pop-outs are full-screen auxiliary windows, so they show over a document in full screen, where an ordinary new window goes to another Space.
- **Also:** a Dock click with no visible window now brings the graphs' windows back, or File › Open with none. The suspected second defect was withdrawn: the maintainer closed the windows.
- **Tests:** an ffi test and a kit test (`watchingAPortPopsItsValueOut`). A scripted run took a pop-out picture with its window `visible: true, onScreen: true, level: 0` (`mac-watched-popout-*.png`; its value shows failed because the sample's Lower case is downstream of its deliberate Fail node).
- **Not verified:** the person-activated path (`makeKeyAndOrderFront`) can't be exercised without activating the app on the maintainer's screen.
- **The iPad** had the same hole and gets the same core fix.

**B. Sliders and precision.**

- **The precision fault was in the core:** `float_decimals` floored the decimals at the current value's own written digits, so a slider's `0.30000000000000004` showed 17 digits. The rule now takes the port's decimals and its step's, never fewer than the default as written or what the value needs read to 10⁻⁹ (at most 6), so float noise never shows.
- **The core gives every number's text** (`Widget.text`): an INT whole and never with decimals, a FLOAT at its decimals. The face, both inspectors, the field editors and the scrub use it, and egui's sheet uses the same rule.
- **Inspector sliders (Mac NSSlider, iPad UISlider):** now continuous; the field beside them follows the drag, and the value is set once at the drag's end (one undo step) on the widget's step and decimals. VoiceOver and keyboard steps set at once.
- **The inspector refreshes:** it reads its rows on every scene change and redraws only when they differ, so an edit on the face, a scrub, undo, redo or a script's `row.set` shows while it is open. A hidden input set elsewhere didn't show before, because only the face's revision was watched.
- **Test:** a conformance case (a FLOAT set to the noise shows `0.30`; an INT set to 6.7 is 7 and shows `7`, in `parameters.get` and the scene).

## egui (item 6, done by a subagent, reviewed by its report and tests)

- **Faces:** drawn at the scene's pushed places (egui-snarl's node positions are writable each frame); a drag moves a face from where it is shown.
- **Frames:** drawn, fitted and handled at their grown size.
- **Backward wires:** taken out of egui-snarl and drawn under the nodes along the core's route, and hit-tested on that route. egui-snarl has no per-wire route hook (its private `draw_wire` in `ui.rs` around lines 1118 to 1185), the same pattern bend-point wires use. No fork and no core change.
- **The limit that remains:** a backward wire can't be picked up by snarl's ⌘-drag from its pin, as bend-point wires couldn't already.
- **Dead rename branches removed:** in `menu_view.rs`, `canvas.rs` (plus `name.port`) and `net_view.rs`. It noted `(None, "canvas.reveal")` and `(None, "wire.unlabelAll")` in `net_view.rs` as probably dead too; I left them.
- **Tests:** 4 new kittests in `crates/ui/tests/kittest_shown.rs` and 2 unit tests. 3 tests in `kittest_moves.rs` now expect drags to start from the shown (pushed) place.

## Core, C ABI, kit and contract changes

- **Session:**
  - `Session::changes`, `mark_autosaved` and `relocate`.
  - New commands `graph.saveCopy`, `graph.duplicate`, `graph.rename` and `graph.move` (in no menu).
  - `app.settings {tab}` takes all seven sections.
- **Editor core:**
  - New modules `autosave` (tick, flush, `document.autosave`) and `settings` (Look, System, model, apply, contrast and opacity, follow, `settings.get`).
  - `Document.look`, `look_seen`, `autosave` and `watch_pops_out`; the scene theme comes from the look.
  - `theme.get` adds `chosen`, `system` and `gen`.
  - The scene's `Widget.text`; `view.windows.settings`.
  - `widget_text::needed_decimals`, `number_text`, and the new `float_decimals` rule.
  - `ui.invoke` continuations for Save a Copy, Move and the theme files.
  - The conformance driver for the core's own document takes a turn when it pauses.
- **Automation:** `Server::start_on(ip, …)`, `is_local`, `lan_address`, and the peer rule beyond loopback.
- **C ABI core** (`crates/ffi/src/core.rs`):
  - Config `dirs.prefs`, `display.{dark, contrast, reduce_motion, reduce_transparency}`, `app` for About, and `control.{bind, token, network_debug}`.
  - The shared look, and the `prefs` feed domain.
  - `document.open {autosave}`, flushes on close and on core close, and `lifecycle.background`, `.foreground`, `.memory` and `.appearance`.
  - `core.control` adds `bind` and `loopback`.
  - No change to `crates/ffi/src/lib.rs` or the header. **`PLUM_ABI_VERSION` is unchanged (1).**
- **Kit:**
  - `SettingsModel` (Kit); `SettingsView` (Shell, SwiftUI).
  - `AutosaveState`, `SystemAppearance`, `ThemeReply.{chosen, system, gen}`, the `prefs` domain, `Windows.settings` and `Widget.text`.
  - `SliderValue`; the canvas sends `lifecycle.appearance`; Reduce Motion in the run pulse.
  - The inspector model reads its rows on every scene change.
  - The conformance runner's `PortDriver(host:)`, the capabilities `port` and `prefs`, `authRefused`, and the CLI's `--host` and `--no-prefs`.
  - `ShellConfig`'s display flags and the debug-only network fields.
  - `app.settings` and `app.about` are off the unbuilt list.
- **Apps:**
  - Mac: `Settings.swift` (the Settings window and About), the menu's About and Settings, `--control-bind`/`--control-token`/`--control-port` (Debug only), the `dirs.prefs` folder, the scratch guard on `PLUMCRAFT_EDITOR_PREFS`, the pop-out frame guard and full-screen behaviour, the Dock reopen handler, and the continuous inspector slider.
  - iPad: `Settings.swift` (the sheet following the core, About), autosave open, close and title, the title menu, the rename delegate, the background flush, the memory warning, the network options, `--open-empty`, a relative `--status-file`, the gear, and the continuous slider. `project.yml` gets the Debug-only plist key.
  - `PlumCraftPad/conformance.sh` now stops when its build fails. It used to run the previously installed app (caught this slice).
- **Contract:** `docs/shell-interface.md`: 4.1 (as built), 4.4 (as built: lifecycle and autosave), 6.3, 7.2, 8.5 (numbers' text), 12.19 and 12.20 (new), 14.2 (as built), 14.4 (new), and 16.1 (41 cases). `docs/control-protocol.md`: beyond loopback, `settings.get`, `document.autosave`. `schema/shell1.json` regenerated; `fixtures/scene/full.json` rewritten (numbers' `text`).

## Conformance

| Driver | Result |
|---|---|
| The core's own document | passes (its look in memory) |
| C ABI headless and its control port (`plumcraft-ffi` `conformance`) | 4 of 4 tests, the whole suite |
| egui editor (`plumcraft-ui` `conformance`) | passes |
| Swift kit (`swift test`) | passes |
| Mac app (`conformance.sh all`) | 41 of 41, 0 skipped |
| iPad app in the simulator (`conformance.sh all`) | 41 of 41, 0 skipped |
| iPad UI tests in the simulator | `testInlineEdits`, `testTouchGestures`, `testWindowsAndNames` pass |
| iPad on the device | not run (locked) |

The five new cases are: settings (the window opened and closed through the core, a setting set, read back, kept and reset); a theme (Dusk, then system, an unknown name refused, back to the default); autosave (saved and matching on disk, a failure reported, off); the port's token (`authRefused`, port drivers); and a number's text.

## Tests

| Run | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean (after fixing an unneeded `mut` in my test) |
| `cargo nextest run --all-features` (b3, scratch) | 1,514 of 1,516 in the gate run: the known local-only `plumcraft-craft::params_coverage::hidden_params_match_the_baseline`, and `shell_schema::checked_in_schema_is_current` (I regenerated the schema before adding `view.windows.settings`; regenerated again, it passes). None of the known intermittents failed |
| `check-hash-gates.sh`, `check-core-toolkit-free.sh` | pass |
| Python | 224 passed, ruff clean (rerun on scratch: see the first section) |
| `build-xcframework.sh` | built |
| `swift build`, `swift test` | zero warnings; 79 of 79 (76 before; 3 new in `SettingsTests.swift`; `ShellTests` changed) |
| Mac Release; iPad simulator Debug and Release; device arm64 Debug | build, zero warnings. Device Release was not built this slice; slice 3's recipe still builds it |
| New Rust tests | session `file_cmds` (3); `autosave` (6); `settings` (4); `widget_text` (1); automation `bind_tests` (2); `crates/ffi/tests/settings_autosave.rs` (5); egui `kittest_shown.rs` (4) and `shown.rs` (2) |

**Changed expectations, deliberately:**

- `kittest_prefs::an_unknown_theme_…`: `view.theme` for an unknown theme is now refused with the reason, rather than accepted and said in the status bar.
- `settings_cmds` test: `colours` is a valid tab now.
- Three `kittest_moves` tests: drags start from where a face is shown.
- `ShellTests.unbuiltCommandsSaySo`: Settings and About are built.

## Preferences, defaults, store, processes

**Preferences and defaults** (checksums before in `claude-debug/native-n4/s4-prefs-before.txt`, after in `s4-prefs-after.txt`):

- The egui editor's, the research software's and a private batch app's `prefs.json` and `trusted.json`, the research software's `virtimp.json`, and the `com.swapwizard.*` plists: identical.
- `com.plumcraft.mac` changed by one key, `NSNavPanelExpandedSizeForOpenMode` (the Open panel's size), written at 20:32:08. That is before my first Mac launch (about 20:46), and a scripted launch never shows an Open panel (`--no-open-panel`), so it was the maintainer's own app.
- `~/Library/Application Support/PlumCraft/` (the new Mac app's own preferences) does not exist: no launch of mine used it.
- The simulator app's preferences lived in its container, which is uninstalled.

**The real store:** see the first section: the eight nonces and `plummemo/ea` are the only differences between `s4-store-before.txt` and `s4-store-after.txt`.

**Processes** (`pgrep -lx` of `PlumCraft`, `PlumCraftPad`, `CanvasSpike`, `CanvasSpikeMac`, `xctrace`; `s4-processes-after.txt`):

- Running now: `5861 PlumCraft` and `58303 PlumCraft`, both the maintainer's.
- 54729, 56625 and 60555 have quit since the start; I never signalled them.
- Nothing else, and nothing of mine.
- No PlumCraft process is listening on any port (`lsof`).

**The simulator** "iPad Pro 13-inch (M5)" is still booted; `com.plumcraft.pad` and the UI test runner are uninstalled, and nothing was erased.

**Disk:** 73 GB free (98 at the start: the xcframework's four targets, the app builds and the device build).

**Conformance, last run:** the Mac app and the iPad app in the simulator, each 41 of 41, 0 skipped, with the final cases.

## Evidence

Under `claude-debug/native-n4/s4-shots/`:

- **Mac, light and dark:** `mac-settings-{appearance, colours, families, files, running, engines, cache}`, `mac-canvas-theme-{paper, pewter, dusk, system}`, `mac-about`, `mac-watched-popout`.
- **iPad simulator, light and dark:** `pad-autosave-{clean, saving, saved, failed}` (the subtitle and the failure's banner), `pad-settings-{appearance, colours, running}`.
- **The UI tests' attachments:** `n4s1-…`, `n4s2-…`, `n4s3-…`.
- **Logs:** `s4-mac-conformance*.log`, `s4-pad-conformance*.log`, `s4-pad-ui.log`, `s4-gate-*.log`, `s4-release-refuses-lan.txt`, `s4-device/` (the device build, install and the refused launch).
- **Not pictured:** the iPad title menu itself (a UIKit menu a script can't open over the port; it needs a UI test or a person), and anything on the device.

## Defects introduced and caught

- The Python gate stage on the real store (above). Not undone, and reported.
- An Info.plist script phase with the same file as input and output failed the simulator build. The iPad conformance script then silently ran the previously installed app and reported 37 of 41. Fixed: a Debug-only build setting merges the key, and the script stops on a failed build.
- A document already dirty when autosave first saw it was saved at once, with no quiet time. Fixed.
- `autosave`'s check that the file on disk matches compared graphs after a JSON round trip, which isn't exact. It now compares the file's text with the graph serialised now.
- The conformance theme case ended by choosing Paper, which an app's kept preferences then held: the simulator's dark pictures show a Paper canvas for that reason. Fixed: the case ends on Match the System.
- The background-task capture under Swift 6, and SwiftUI's `View` clashing with the kit's `View` type. Fixed.

## Decisions for the maintainer

1. **The Mac app's preferences file** is its own (`~/Library/Application Support/PlumCraft/prefs.json`), not the egui editor's (`plumcraft-editor/prefs.json`). Alternative: share one, so a theme chosen in either shows in both.
2. **Autosave timing:** 1 s quiet, at most 10 s, waiting for a run to end, retrying after 5 s doubling to 60 s. Alternative: save during a run too, since the graph snapshot is consistent anyway.
3. **A watch pops the value out** in the Apple apps (a click on a pin, every Watch item); egui keeps its panel. Alternative: only Watch items pop out, and a click only watches.
4. **Settings on the iPad:** an in-app sheet plus a gear in the bar, and no Settings app bundle. Alternative: no gear (the app menu only).
5. **Zoom not offered** in the Apple apps, and **Text size** not yet applied to the Apple canvas's fonts. Build it (re-measure every face on change) or drop it from those apps' model.
6. **Move… on the iPad** goes through the `folder.pick` ask, and the core moves the file. On a provider that only grants access to the file, the move can fail with the reason; untested on a device.
7. **The real store's stray entries** (first section): remove them, or leave them (they are a handshake's nonces and one memo).

## What only a person can verify

- **On the Mac, with the app active:** the pop-out after a click on a pin coming forward and becoming key; the Settings window's tabs and controls (colour wells, sliders, Import/Export panels); ⌘,; the About panel; the full-screen case.
- **On the iPad:** the settings sheet, rename in the title, Duplicate, Save a Copy…, Move… with the Files picker, backgrounding during a pending save, a memory warning, the local-network prompt, and the suite on the device once it is unlocked (`s4-ipad-device.sh`).

## For slice 5 (the remaining viewers by note 24 §8's order)

- Pop-outs now open from every watch in the Apple apps, so a viewer for `items`, `array.pictures` and `table.grid` is reached often. Until each is built, the window says "no viewer yet".
- `Widget.text` is the core's number formatting. Viewers that show numbers (`table.grid`, `array.grid`) should take their text from the core the same way, not format in the shell.
- The look is shared: a viewer drawing in theme colours reads `theme.get` again on the feed's `prefs` domain.
- The device suite is still owed: `s4-ipad-device.sh` with the iPad unlocked.
