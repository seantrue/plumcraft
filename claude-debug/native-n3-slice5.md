---
title: "N3 slice 5: the iPad app, read and run, and the N3 device budgets"
date: 2026-11-20
---

# N3 slice 5: the iPad app, read and run, and the N3 device budgets

Branch `native/n3-ipad`, cut from `origin/feat/native-n0`. Nothing is committed; the changes are in the working tree for review. `AI.md` and `docs/BUGS.md` were not touched.

## Summary

| Item | State |
|---|---|
| 1. Open and show | Done: document browser root, Files, open in place, security-scoped access, UIDocument read, bundled samples (Showcase opens on first launch), state restoration (graph and view), light and dark, Dynamic Type in the app's own text, safe areas, every orientation (checked by rotation in the UI test), compact width (Watched panel as a sheet), trackpad scroll, pinch and hover |
| 2. Pan and zoom by touch | Done: one finger on empty canvas, two fingers anywhere, momentum, rubber band, pinch about the fingers, double tap (below), tap selects, tap on a pin watches. Checked through UIKit's recognisers by an XCUITest |
| 3. Run and watch | Done: both engines in process, engine state in the bar, Run, Run Selected, Cancel, the wash, a Watched inspector (column in regular width, sheet in compact) with `picture`, `text.plain`, `scalar` |
| 4. Menus and asks | Done, with explicit Save (a decision, below): the iPadOS menu bar and shortcut overlay from the core's model, canvas keys, context menus on long press and secondary click, the asks natively, the share sheet for `share` |
| 5. Logging and debug driving | Done for the simulator: the core's log in the unified log; the control port drives the simulator app. Not on a device: the C ABI binds loopback only (decision 10's local-network binding is not built) |
| 6. Conformance | Read half 12 of 12, no skips, in the simulator by one command; edit half 12 of 12; the Mac's read half 12 of 12 as before |
| 7. Budgets on device | Measured on the maintainer's iPad Pro 11-inch (M4): launch and the one-node change at 500 nodes met; at 2,000 not; the display ran at 60 Hz, never 120; hitches at 500 well over budget. The Mac's were re-read on a loaded machine and can't be trusted |
| 8. The N3 gate | Not met as a whole: see "The N3 gate" |

The 35 ms at 2,000 nodes was profiled (a subagent, `claude-debug/native-n3/s5-prof-*`), and one cause in the core was fixed: graph validation was quadratic in wires.

## 1. The iPad app

`native/apple/PlumCraftPad` (XcodeGen; the generated project, `App/Info.plist`, `build/` and `build-device/` are ignored). Bundle id `com.plumcraft.pad`, iPad only, iOS 18, Swift 6, warnings as errors, no third-party code.

| File | What |
|---|---|
| `App/AppMain.swift` | Launch options, the container's folders, the core's start (store and preferences forced into the container), the menu bar hook, memory warnings |
| `App/Documents.swift` | `GraphDocument` (UIDocument, reads and checks only), samples, `BrowserViewController` (UIDocumentBrowserViewController), `SceneDelegate` (opening, URL contexts, state restoration) |
| `App/GraphViewController.swift` | One graph: canvas, bar, Watched panel placement, asks, running, saving and closing, canvas keys, menu validation, status for scripts |
| `App/Watched.swift` | The three viewers |
| `App/Asks.swift` | The asks |
| `App/Menus.swift` | The menu bar from the model (`UIMenuBuilder`), key commands, context menu actions |
| `App/Script.swift`, `App/Bench.swift` | `--then` steps for evidence runs; the budget run |
| `UITests/GestureTests.swift` | XCUITest of the recognisers |
| `conformance.sh`, `bench.sh` | One command each |

**Documents.** Each scene's root is the system's document browser (Recents, Shared, Browse, iCloud Drive, On My iPad, tags, search, New Graph). Picking a graph opens it in place: the URL's security-scoped access is started and kept until the graph closes, a UIDocument reads it (coordinated, downloaded first from a cloud provider) and checks it is a graph, and the core opens the same path. The graph replaces the browser as the window's root and gives it back when it closes ("Documents" in the bar). Several scenes work (decision 8): one core, a core document per scene. State restoration saves a bookmark of the graph and the view's pan and zoom; reconnecting reopens both. On the first launch the samples (`Showcase`, `Frames and Groups`) are copied into Documents/Samples and Showcase opens.

**Look.** System colours and fonts, Dynamic Type for the panel's and the bar's text (the canvas's face text is the contract's sizes, as on the Mac), safe areas (the canvas starts under the bar), light and dark (the canvas follows: Paper and Dusk), all four orientations allowed, Split View and Stage Manager sizes allowed (`UIRequiresFullScreen` false). A resize keeps what was at the canvas's centre at its centre. In compact width the engine word goes and only its dot stays.

**The bar.** Leading: Documents, Fit. Title: the graph's name with a menu (Save, Share…, the system's), and as subtitle "Edited" and the zoom. Trailing: Run, Run Selected, Cancel Run, the engines' dot and word, the Watched panel.

## 2. Gestures

In the kit's UIKit host (`PlumCraftCanvas/CanvasView+UIKit.swift`), so the canvas is the Mac's and only recognition is the iPad's.

| Gesture | What it does |
|---|---|
| One finger on empty canvas (or a wire, frame or tag) | Pans, with momentum (UIScrollView's normal deceleration) and the rubber band. On a face or a pin it doesn't begin: that is N4's drag |
| Two fingers anywhere | Pans, together with the pinch |
| Trackpad two-finger scroll, mouse wheel | Pans (scroll events only), with momentum on lift |
| Pinch (fingers or trackpad) | Zooms about the pinch's centre |
| Tap | `ui.gesture` tap with the keyboard's modifiers: the core selects, adds, or watches a pin |
| Double tap | On a face: fits it; a second double tap goes back. On empty canvas: fits everything |
| Long press, secondary click | The core's context menu (a deferred menu filled when `menu.get` answers), lifting the face under the press |
| Pointer or Pencil hover | Rings the pin or widens the wire under it |

**The double-tap conflict (left for the maintainer).** The iPad spike's rule makes a double tap on empty canvas Fit; decision 6 makes it the palette, which is N4's. I built the spike's rule, plus 5.5's "double tap fits a node" on a face. So the core's own `doubleTap` ("opens what it is on", e.g. entering a group) is not reached by touch in this slice; a person reaches those through the context menu. The conformance case for it passes over the port.

**Verified through UIKit, not just the core.** `conformance.sh gestures` runs an XCUITest in the simulator: a tap on Replace selects it, a tap on empty canvas clears, a long press shows the core's menu for Replace (screenshot `s5-ui-context-menu.png`), a dismissal, a pinch zooms in, a one-finger drag on empty canvas pans and leaves the zoom, a double tap on empty canvas fits, then a rotation to landscape and back keeps the graph shown. Passed (`claude-debug/native-n3/s5-pad-gestures-final.txt`). Two-finger pan, momentum, hover and a real trackpad can't be driven by XCUITest and were not exercised.

## 3. Run and watch

As the Mac: every graph opens on the text engine with the array engine beside it (`engines.also`). Run is the model's `run.start`, Run Selected is `run.start {targets}` (shell-side, as on the Mac), Cancel is `run.cancel`; each is enabled from the model. The wash, pulse, hold and fade are the kit's canvas. The **Watched panel** is an inspector column on the right in regular width (280 to 360 points), toggled from the bar or Option-Command-I; in compact width it is a sheet with medium and large detents (medium undimmed, so the canvas stays usable). Swiping the sheet down hides the panel. Viewers: `picture` in a zoomable scroll view (pinch, double tap toggles fit and 1:1, Fit and 1:1 buttons, centred, nearest-neighbour when magnified), `text.plain` in a selectable text view with a monospaced switch and Show All (the whole value's bytes), `scalar` large and selectable. A failed value says the engine's message.

## 4. Menus, keys and asks

- **The menu bar** (iPadOS 26's, and the shortcut overlay when Command is held) is built by `UIMenuBuilder` from the front graph's menu model, rebuilt only when its structure changes, revalidated otherwise. File, Edit and View take the model's items; Node, Run, Group, Engine follow View; Format is removed. A disabled item shows the core's reason as its subtitle. Undo and Redo carry the step's name. Cut, Copy, Paste, Delete and Select All are the standard edit actions (a text field takes them; the canvas sends the core's commands). A model shortcut is a key command only with a modifier besides Shift, and never twice. View adds Zoom In (Command-plus), Zoom Out, Actual Size and Show Watched Panel.
- **Canvas keys** (an external keyboard): F fits, Delete and Backspace delete, Escape cancels a run or clears the selection, the arrows nudge (Shift ten times). UIKit doesn't mark a key command's repeats, so a press within half a second of the last joins its undo step.
- **Items whose UI isn't built** (Settings, Edit Parameters, value editors, pop-outs, Messages, the palette, Attach, Install, Compare) say so in a banner, from the kit's list shared with the Mac. Open and New go to the document browser.
- **The asks:** see the table in the contract's 11.2 note. Modal asks one at a time over whatever the graph shows; a script's asks are left for the script.
- **Saving: explicit, as a decision.** Decision 7 says iOS autosaves with no Save command; the contract puts it in the core (`document.open {autosave: true}`, 4.4), which isn't built, and a UIDocument autosave driven by the shell would write the file behind the core's dirty state. So the core's Save (File › Save, Command-S, the title menu) saves, the subtitle says "Edited" while the core says dirty, and closing an edited graph asks Save, Don't Save or Cancel. The UIDocument never saves.

## 5. Logging and debug driving

- **Logging:** the kit's `UnifiedLog` reads the core's log (`log.tail` on the feed's log mark) into os_log under `com.plumcraft.pad`, category = the entry's source; the app's own lines go to os_log and to the core's log (`log.write`). Seen with `xcrun simctl spawn <udid> log show --predicate 'subsystem == "com.plumcraft.pad"'` (`claude-debug/native-n3/s5-unified-log-sample.txt`: the app's start line with its store, the core's `ffi` lines, banners). On a device the same lines are in Console or `log collect`; I didn't read a device's log.
- **Debug driving:** `--control-token-file` opens the core's port, which the simulator shares with the Mac; `--status-file`, `--then` steps and the UI test drive the rest. **On a device: not built.** The C ABI binds 127.0.0.1 only; decision 10's debug binding to the local network needs a core change (a bind address) and a security review, so it is a decision before N4. The device runs below were driven by launch arguments and `devicectl`, not the port.

## 6. Conformance

| Run | Passed | Skipped | Log |
|---|---|---|---|
| iPad app, simulator, read half (final build) | 12 of 12 | 0 | `claude-debug/native-n3/s5-pad-conformance-read-final.txt` |
| iPad app, simulator, edit half | 12 of 12 | 0 | `s5-pad-conformance-edit.txt` |
| iPad app, simulator, gestures (XCUITest) | passed | | `s5-pad-gestures-final.txt` |
| Mac app, read half | 12 of 12 | 0 | `s5-mac-conformance-read.txt` |

Settle timeouts 0; the read half takes about 0.7 s. `conformance.sh` checks the app's store is inside its container before it runs the cases.

## 7. Budgets

### The device

The maintainer's iPad Pro 11-inch (M4), UDID 00008132-001424640C05801C, connected and unlocked. Signed with the team's existing "Apple Development" identity and the profile Xcode already had for it (automatic signing without `-allowProvisioningUpdates`; Xcode's team wildcard profile, which covers this device; nothing was requested from Apple). A first attempt with manual signing was refused by Xcode ("is Xcode managed") before anything was built or installed.

**What was installed and launched, all 2026-11-20:** `PlumCraft` as `com.plumcraft.pad` 0.7.0, installed at 13:17 and reinstalled at 13:18, 13:20 (the build with the core fix), 13:20:47 and 13:24 (`bench.sh` installs before each run). Launches: 13:17 (bench, 500), 13:18 (bench, 2,000), 13:20 (bench, 2,000, core fixed), 13:20:47 (a trace that named the app by its Mac path, which recorded nothing usable), about 13:21 (Animation Hitches trace, 500, by bundle id), 13:24 (trace, 2,000), 13:28 (the N0 spike `com.plumcraft.spike.canvas`, already installed, traced at 500 as a control). Each bench exits by itself when done (exit code 0); the traced runs ended with their recordings. At 13:32 no PlumCraftPad or CanvasSpike process was running on the device (`devicectl device info processes`). The spike and the research software's spike app were left as they were. `com.plumcraft.pad` stays installed on the iPad.

| What | 500 nodes, 800 wires | 2,000 nodes, 3,200 wires (before the core fix) | 2,000 (after) | Budget |
|---|---|---|---|---|
| Launch to first frame | 0.49 s | 1.77 s | 1.55 s | under 1 s |
| `document.open` to first frame | 403 ms | 1,503 ms | 1,456 ms | |
| The core's `spike.scene` build | 299 ms | 817 ms | 788 ms | |
| One-node change on its layer (kit), p50 | 12.5 ms (p95 16.1) | 73.5 ms | 57.3 ms | 16 ms |
| The same through the feed, p50 | 17.7 ms | 62.3 ms | 50.3 ms | 16 ms |
| Footprint at the end | 67 MB | 241 MB | 236 MB | |
| Display link | 60 Hz every frame | 60 Hz | 60 Hz | 120 Hz |

**Hitches on the device at 500 nodes** (Instruments' Animation Hitches, `claude-debug/native-n3/s5-pad-device-trace-500-hitches.md`):

| Phase | Hitch ms a second | Longest |
|---|---|---|
| idle | 11.1 | 16.7 |
| pan | 136.7 | 41.7 |
| pan at fit | 341.4 | 25.0 |
| zoom | 116.8 | 33.3 |
| drag | 30.3 | 20.8 |

Instruments' reasons: "Potentially expensive render, potentially expensive GPU work". So it is the render server and the GPU, not the main thread: 500 face layers with full-face bitmaps (123 MB of face pictures at the device's scale 2) composited every frame. The 2,000-node trace recorded 214 hitches (11.6 s) but no phase signposts came out of it, and the spike's control trace failed to save, so there is no same-day device control.

**Can these be trusted?** The launch, one-node and memory rows: yes, the device was otherwise idle and each is a median of 20. The hitch rows: they are one run without a control, but large (pan at fit 341 ms a second against a budget of 5), so the budget is missed at 500 nodes on the device whatever the noise. The 60 Hz: the display link asked for 120 (`preferredFrameRateRange` 60 to 120, preferred 120) and the screen reports 120 maximum, but every frame came at 16.67 ms, as N0 saw. I could not see the cause: Low Power Mode or Settings › Accessibility › Motion › Limit Frame Rate on the iPad would do it, and only a person holding the iPad can check. So "every frame at 120 Hz on an iPad" is still unmeasured.

### The simulator

Not a budget measure (its timings are the Mac's). The app opened Showcase to its first frame in 0.4 s (`firstFrameMs` 391 in the UI test's status).

### The Mac

Re-read with the core fix; **not trustworthy**. The load average was 8 to 11 throughout (Spotlight's knowledge updater at 126 % CPU, Time Machine, the simulator), against about 5 in slice 4. I did not take Mac traces or run the spike as a control for that reason.

| What | 500 | 2,000 | Slice 4 (500; 2,000) |
|---|---|---|---|
| Launch to first frame | 0.57 s | 1.17 s | 0.78; 1.20 s |
| One-node change (kit), p50 | 5.7 ms | 130.6 ms (second run 168) | 6.2; 35.5 ms |
| Through the feed, p50 | 20.3 ms | 131.5 ms | 18.2; 36.1 ms |

The 2,000-node change reads 130 to 170 ms here, with or without culling (`s5-nocull-bench-2000.json`), against the kit alone at 20.5 ms (`swift test` measurement, same hour) and the core's own at 19 to 20 ms. Something between the model and the layer costs over 100 ms on this loaded Mac, where slice 4 read 35. A `sample` taken during the run (`s5-mac-sample-2000-onenode.txt`) caught mostly idle threads, so I can't say what. It must be read again on a quiet machine before anything is concluded; it may be a regression of mine, though nothing I changed is on that path (the bench moved into the kit unchanged, and the canvas is unchanged on the Mac).

## Where the 35 ms at 2,000 nodes goes

Profiled in the core with `sample` on release builds (`claude-debug/native-n3/s5-prof-baseline.txt`, `s5-prof-sample-text2000.txt`; measurement `crates/ffi/tests/measure_node_set.rs`, ignored by default). Before the fix, at 2,000 nodes (text engine; medians): the `node.set` reply 24.1 ms, then `scene.get {since}` 6.5 ms, so 30.7 ms from the set to the delta; the canvas's share made up the rest of slice 4's 35.

| Where | Cost | Done |
|---|---|---|
| `Graph::check` → `wire_problems` → `output_is_list` per wire, each restarting the mapped-node walk (wires × wires, mostly id compares) | about 10.6 ms | **Fixed:** the mapped nodes are worked out once per check |
| The turn after the command: `note_scene` builds the whole scene (about 4 ms), publishes and diffs (1 ms), rebuilds the hit-test snapshot (1.6 ms), before the reply's wake | about 7 to 8 ms | Proposal 2 |
| `scene.get {since}` builds the whole scene again, though `note_scene` just did | about 4 to 6 ms | Proposal 1 |
| `RunState::on_edit` (everything downstream goes stale) | about 2.4 ms | No |
| The undo step's graph clone and push | about 0.7 ms | No |
| Waking a core that was idle | 2 to 4 ms | No |

After the fix: the reply 15.1 ms and set to delta 22.5 ms (text), 15.9 and 24.6 ms (text and array). Through the kit, 20.5 ms end to end at 2,000 (`kit.delta.2000`, measured this afternoon on the loaded Mac). On the iPad it went from 73.5 to 57.3 ms.

The second engine costs node.set about 5 %, not the catalogue lookup. Slice 4's extra 260 ms on the first big edit is waiting for the array engine to connect: document requests wait until both engines are ready (`ready_ms` 272 with both, 9 with text alone).

**Proposals, not done (larger):**

1. Don't build a scene that hasn't changed: cache the built scene, keyed on everything it reads (session and graph generations, run state, selection, view, theme, focus, geometry), so `scene.get {since}` after `note_scene` costs only the diff. Saves 4 to 6 ms per edit and per busy turn.
2. Wake the shell as soon as a reply is queued, before the turn's scene work: the reply arrives about 7 ms sooner; it helps the screen only with proposal 1.
3. Smaller: `on_edit`'s wire clones and `catalog.node()`'s linear scan, each under 1 ms.

At 2,000 nodes the 16 ms budget needs 1 and 2 at least, and the device needs a faster canvas commit as well.

## The N3 gate

N3's gate (note 24 section 12): "the read-only half of the suite on both; budgets met on device".

| Part | Mac | iPad |
|---|---|---|
| Read half of the suite, no skips | **Met:** 12 of 12 | **Met** in the simulator: 12 of 12. Not run on the device (no control port there) |
| Launch to a usable window under 1 s | Met at 500 (0.57 s); 1.17 s at 2,000 (loaded machine) | **Met at 500** (0.49 s); **not at 2,000** (1.55 s) |
| A command to its scene change on screen, 16 ms | Met at 500 through the kit (5.7 ms), not through the feed (20.3 ms); 2,000 unreadable (130 ms on a loaded machine) | **Met at 500 through the kit** (12.5 ms p50, 16.1 p95), not through the feed (17.7); **not at 2,000** (57.3 ms) |
| Every frame of pan, zoom, drag at 120 Hz, 500 nodes | **Unmeasured** (no trustworthy trace this slice; slice 3 and 4 noisy) | **Not met** at 60 Hz (pan 137, pan at fit 341, zoom 117, drag 30 ms of hitches a second); **120 Hz unmeasured** (the display ran at 60 Hz) |
| Run progress, one change a frame | Coalesced in the core (unchanged) | The same |
| Main-thread stall over 100 ms | None seen | None seen in the evidence runs; not traced |
| Memory on a warning | No `lifecycle.memory` in the core; the iPad drops its decoded pictures | The same |

**So N3's gate is not met**: the read half is met on both (the iPad's in the simulator), and the device budgets are met only for launch and a kit-driven edit at 500 nodes. The canvas's frame budget on the iPad is missed by a wide margin at 60 Hz and unmeasured at 120.

## Every Rust, C ABI, kit and contract change

**Rust** (from the profiling, by a subagent, reviewed): `crates/graph/src/lib.rs`: a private `Graph::mapped_walk` (the old body of `mapped_nodes`, which also says whether the walk reached every node; `mapped_nodes` calls it), and `Graph::wire_problems` works the mapped set out once and uses it for each wire, falling back to the per-wire question on a (corrupt) cycle, so the answer is the same in every case. New test `wire_problems_find_what_asking_each_wire_found` (a clean graph, a problem two nodes downstream after a node stops being mapped, a cycle) against a per-wire reference. New ignored measurement `crates/ffi/tests/measure_node_set.rs`. **No C ABI export, struct, JSON shape or message changed.**

**Kit (`native/apple/PlumCraftFFI`):**

- **New target `PlumCraftShell`**, what the two apps share above the canvas: `ShellConfig` and `ShellCore.open` (the config of contract 4.1 and the start that closes the core's own first document), `StatusFile`, the menu model's lookup (`[MenuItem].item(command:)`), `ShellCommands` (the commands with no UI yet and what to say), `AskWords` (the answers' words, a file ask's extensions), `ValueText` (a scalar's text, the summary, the excerpt, the whole text from the value's bytes), `CanvasBench` (the budget run, both platforms) and `UnifiedLog`. Tests `PlumCraftShellTests` (8).
- **`DocumentModel.refreshEngine()`**, called by `start()`: reads the engine's state once from `ui.inspect`. A defect: a model started after its engines connected never heard they were ready (the feed said so before it listened), so the iPad showed "Engines starting…" for ever. Test `aModelStartedLateKnowsItsEngine` (fails without the fix, passes with it).
- **The UIKit host** (`CanvasView+UIKit.swift`) rewritten as section 2 says, with theme following, context menus, hover, momentum, `snapshotPNG()`, and a resize that keeps the centre.

**Moved from the Mac app into the kit:** the core's config and start (`CoreHost.open`), the status file (`Status`, now `StatusFile` behind a typealias), the bench (`Bench.swift` is now a window around `CanvasBench`), the unbuilt-command list and its banner text, the answers' words and the file asks' extensions, the scalar's and the summary's text and the whole-text fetch, and the toolbar's model-item lookup. The Mac app's behaviour is unchanged (its conformance and a clean release build pass); its Watched panel now says a failed value's message rather than its JSON.

**Contract (`docs/shell-interface.md`):** "as built (N3 slice 5)" notes in 4.1 (the iPad's config), 11.2 (the iPad's answers), 15 (the unified log through `log.tail`) and 16.1 (conformance and gestures against the iPad app). `docs/control-protocol.md` and `schema/shell1.json` don't change. Ledger: note 24 section 18 gains slice 5 and "N3's exit"; `docs/STATUS.md` an entry.

## Tests

| Suite | Result |
|---|---|
| `cargo fmt --all --check` | Clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | Clean |
| `cargo nextest run --all-features --no-fail-fast` (b3, scratch) | **1,467 of 1,468 passed**, 17 skipped; the one failure is the known `plumcraft-craft::params_coverage hidden_params_match_the_baseline`. One new test (`s5-gate-nextest.log`) |
| `scripts/check-hash-gates.sh`, `scripts/check-core-toolkit-free.sh` | ok, ok |
| `native/apple/build-xcframework.sh` | Built, all slices, with the core fix |
| `swift build`, `swift test` (scratch) | Zero warnings; **64 tests passed** (55 before, 8 shell tests, 1 model test), measurements opt-in as before (`s5-swift-test.log`) |
| Mac app, clean release build | Zero warnings (`s5-mac-clean-build.log`) |
| iPad app, clean release build, simulator and device (arm64) | Zero warnings (`s5-pad-clean-build-sim.log`, `s5-pad-clean-build-device.log`) |
| Conformance and gestures | As in section 6 |

The subagent also saw `prefs::tests::each_application_has_its_own_file` fail when `PLUMCRAFT_EDITOR_PREFS` is set in the environment (the brief's scratch environment sets it); it passes without, and the gate's run (which doesn't set it) passed it.

## The maintainer's store, preferences, simulator

- **The store** (`/Volumes/CommonkitStore/store`, top two levels, names, sizes and mtimes): **no difference** between 12:45 and the end (`claude-debug/native-n3/s5-store-before.txt`, `s5-store-after.txt`). The research software's 12:04 writes are before the first listing.
- **Preferences and trusted files** of the editor, the research software's app and PlumPipe: the same SHA-256 before and after.
- **The iPad app's store** in the simulator was inside its container in every run (checked by `conformance.sh` and the status files).
- **The simulator:** still booted, appearance light as found, the same 40 apps as before (`s5-sim-apps-before.txt`, `-after.txt`; `com.plumcraft.pad` and its UI-test runner uninstalled at the end). Not erased.

## Final process check

```
$ pgrep -lx PlumCraft
$ pgrep -lx PlumCraftPad
$ pgrep -lx CanvasSpike
$ pgrep -lx xctrace
```

All four empty. Nothing of mine runs on the iPad.

## Screenshots

Under `claude-debug/native-n3/s5-shots/`, from `simctl io screenshot` of the iPad Pro 13-inch (M5) simulator (2064 × 2752), or the UI test's own screenshots.

| File | What |
|---|---|
| `s5-first-launch.png` | A fresh install's first launch: Showcase opened from the samples |
| `s5-document-browser.png` | The document browser after closing it |
| `s5-light-fit.png`, `s5-dark-fit.png` | Fitted, light and dark, portrait, Watched panel open |
| `s5-light-zoomed.png` | Zoomed to 160 % on Replace |
| `s5-light-mid-run.png`, `s5-dark-mid-run.png` | Mid-run: Slow running with its progress, others queued, Fail in error |
| `s5-light-after-run.png`, `s5-dark-after-run.png` | After the run, previews on the Mandelbrot and Encode Image faces |
| `s5-light-error.png` | The failing node watched: its error on the face and in the panel |
| `s5-light-watched-picture.png`, `s5-dark-watched-picture.png` | The picture viewer |
| `s5-light-watched-text.png` | The text viewer |
| `s5-light-watched-scalar.png`, `s5-dark-watched-scalar.png` | The scalar viewer |
| `s5-dark-compact-sheet.png` | Compact width (the size class forced, below): the Watched panel as a sheet at its medium detent |
| `s5-dark-regular-again.png` | Back to regular width: the panel a column again |
| `s5-ui-context-menu.png` | A real long press (XCUITest): the core's context menu for Replace, the face lifted |
| `s5-ui-landscape.png`, `s5-ui-rotating-portrait.png` | Landscape, and portrait again, from the final UI test run (the graph kept its centre); `s5-ui-upside-down.png` is from a failed run and shows the defect below (the browser) |
| `s5-ui-after-gestures.png` | After the gesture test |

`s5-light-context.png` and `s5-try1.png` are from discarded attempts (the first shows no menu: a scripted menu button didn't open).

**Compact width** was forced with the window's `traitOverrides` (horizontal size class compact) at full width: the simulator's Split View can't be scripted. A real Split View or narrow Stage Manager window wasn't seen.

## Defects I introduced and caught

1. **Rotating closed the graph from view.** The graph was presented full screen over the document browser, and a rotation dismissed the presentation, leaving the graph open in the core behind the browser. Caught by the UI test's rotation (its screenshots showed the browser). Fixed: the graph replaces the browser as the window's root and gives it back on close; the UI test now checks the graph is still shown after rotating.
2. **A crash going from compact to regular width:** the panel was made a child while still presented as a sheet (`UIViewController` threw). Caught by the dark evidence run stalling, then the crash report. Fixed: it leaves the sheet first.
3. **"Engines starting…" for ever** (the kit's model missed the engine's ready state). Caught by the first simulator status. Fixed in the kit with a test.
4. **Scenes from earlier runs came forward** over a script's graph. Fixed: a scripted launch (`--scratch-state`) keeps one scene.
5. **A double tap at a "corner" landed on a face** in the first UI test (the point wasn't empty). Caught by the test; the point moved.
6. **The hitch reader found no phases** in a device trace (it read the wrong table). Fixed in a copy, `claude-debug/native-n3/s5-hitches.py`.
7. **Device signing:** my first device build asked for manual signing with the Xcode-managed profile, which Xcode refused before building. Switched to automatic signing without provisioning updates. Nothing was installed by the failed attempt.
8. **A scripted context-menu step** (a hidden menu button) showed no menu; replaced by the UI test's real long press.
9. **Swift 6 isolation errors and a force unwrap** in early drafts, caught by the compiler and by review.

## Decisions for the maintainer before N4

From this slice:

1. **Double tap on empty canvas:** Fit (the spike's, built now) or the palette (decision 6, N4). And on a face: fit it (built, 5.5's zoom row) or the core's "open what it is on" (enter a group, a value's viewer)?
2. **Saving on iPad:** build the core's `document.open {autosave: true}` (contract 4.4, decision 7), or keep explicit Save (built now). A shell-side UIDocument autosave is the third option, not recommended.
3. **The control port on a device** (decision 10): a bind address in the C ABI's config for debug builds, with the token, or simulator only.
4. **The iPad canvas's frame budget:** 500 nodes hitch heavily on the device in the render server. Options: smaller face bitmaps (scale 1 below zoom 1, the zoomed-out face of 15.2), fewer layers (culling helps only off screen), or rasterising groups of faces. N4 should start with this.
5. **120 Hz:** check the iPad's Low Power Mode and Limit Frame Rate, then measure again.
6. **The core's per-edit scene work** (proposals 1 and 2 above) for 2,000 nodes.
7. **`share`:** declare `shell.share_targets` (`[["more", "Share…"]]`) so the core offers Send To, now that the iPad answers it.
8. **Run Selected** as a core command (`run.selected`), as slice 4 asked.
9. **The bundled samples** and opening Showcase on first launch.
10. **The Watched panel's width and the compact sheet's detents** (medium undimmed).
11. **`max_picture_px` 2048** on the iPad (the contract's example says 1280).
12. **Keeping both engines in every document**: it costs about 270 ms at launch on the Mac (waiting for the array engine), more on the iPad.

Still open from slices 3 and 4 (look and behaviour): own transform rather than a scroll view, with the rubber band; smart zoom's toggle; drags on faces in the Mac's read-only app; window tabs and the Open panel at launch; the face's look (no Lucide icons, selection outline crossing pins); following the system's appearance (not kept, Pewter unreachable); header parts not reported; labels drawn at twice the screen's scale; `public.json` at rank Alternate; Undo from the model rather than `UndoManager`; the engine state as a toolbar item; items with no UI shown as a banner; plain keys not menu equivalents; the wash tinting the text; the panel showing the engine's rendition, not a view-sized picture; undo back to saved keeps the dot; the panel starting open.

## What I did not verify (and what only a person with the iPad can check)

- **With the iPad in hand:** finger pan with momentum, two-finger pan, pinch, long press and the menu's lift, hover with a Pencil or trackpad, the Magic Keyboard's menu bar and shortcut overlay, Command-S, the canvas keys, Split View and Stage Manager sizes, the Files app opening a graph from iCloud Drive (open in place and the security scope), state restoration after the system kills the app, Dynamic Type at large sizes, the share sheet, and why the display link ran at 60 Hz (Low Power Mode, Limit Frame Rate).
- **On a device generally:** the conformance suite (no port), the unified log read from the device, a hitch trace at 2,000 nodes with phases, the spike as a same-day control.
- **The Mac's budgets on a quiet machine,** and the 130 ms one-node change at 2,000 there.
- **The asks by a person:** the document picker's answers, the save alert, text and form alerts, open.external; built, exercised only by type-checking and the conformance suite's script answers.
- **Several scenes at once** (two graphs in two windows): built, not exercised.
- **Linux:** the Rust change ran on the Mac only; the controller runs the gate on the Linux test host.
