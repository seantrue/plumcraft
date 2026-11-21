# N4 slice 6: the rest of the banner, iPhone, the floor walk and N4's exit

Branch `native/n4-gate` (from `origin/feat/native-n0` with N4 slices 1 to 5). Nothing committed: the controller reviews and commits.

## First: what appeared or sounded on the Mac, and writes outside scratch

Nothing modal appeared and nothing sounded. Every Mac launch was scripted (`--scratch-state`), never activated, killed by the PID captured at launch; the new Cache Report, Parameters and Compare windows are ordered just above their own document window, as slice 5's pop-outs are. `screencapture -l` answered "could not create image from window" for every Mac window this slice (the display was asleep or locked, as in slice 5's later runs); nothing was woken, so there are no Mac pictures, only the Mac app's own reports of the windows it opened (`s6-shots/mac-report-*.json`). The maintainer's `PlumCraft` processes were never signalled; nothing was built into `native/apple/PlumCraftMac/build` or touched under `claude-debug/native-n3/s6-try/`.

Writes outside scratch: the source tree (below), the iPad project's own `build/` (allowed) and the kit's `.build`. Nothing in the real store or preferences (below).

## B41: cause and fix

**A product defect, not the test's.** A C ABI core's documents share one look (`settings::Look`). When a run setting is changed in a document, `settings::after_command` wrote **all three** run settings (colours, animation speed, Pick Engines Automatically) from that document into the preferences (`keep_run_prefs`). A document that had not yet followed the look (the core's tick hadn't reached it since another document's change) wrote its stale copy back. Every document runs `engine.auto` itself once its first engine is there (`document::settle_startup`), so in the test the second document (`document.open`), whose text engine connected after the first document had set Animation Speed to 1000, wrote back `anim_ms: None`, and the next follow (the appearance change) put 500 back into the first document. On the Linux test host the engine connects later relative to the requests (80 cores, other tests), so the window was hit about 6 runs in 10; on the Mac the second document was usually ready first. The same loss happens to a person with two windows: change Animation Speed in one while the other is still starting.

**Fix at source** (`crates/editor-core/src/settings.rs`, `prefs.rs`): `after_command` keeps only the setting the command changed (`Look::keep_run_pref(RunPref::{Colors, Anim, Auto})`, `Store::set_run_colors`, `set_anim_ms`), then the document follows the look for the other two. `settings.get {fromDisk: true}` now reports the run settings as the file holds them, not the document's (the second suspect). A double lock I introduced on the way (the look locked twice in `after_command`) hung the FFI test; caught and fixed before anything else ran.

**Test that fails without the fix:** `settings::tests::a_run_setting_changed_in_one_document_survives_another_that_has_not_followed_yet` (two documents on one look; A sets 1000; B, not yet followed, runs `engine.auto`; the preferences, the file and `settings.get {fromDisk}` must say 1000). With the old write-all-three restored it fails ("b wrote its stale animation speed back"); with the fix it passes. No dependence on the tick's timing. The FFI test passed 200 of 200 under `--stress-count 200` on the Mac afterwards. I could not run it on the Linux test host.

## Items 2 to 7

| Item | Done? | What |
|---|---|---|
| 2. Visual defects | Done | iPadOS 26 resolves `.systemBackground` and `.systemGroupedBackground` see-through inside a partial-height sheet, so slice 5's "opaque" sheet wasn't: every sheet (pop-outs, the new report sheets, Messages, Settings, the parameters sheet, the palette's compact sheet) now has an explicit opaque ground, light and dark. The empty band was the summary label taking the sheet's spare height above a viewer of fixed height (the sound's): it now hugs its text. The table's far-right start was SwiftUI `Table` on iOS: replaced on iOS by a grid of the page's own column widths from the leading edge (header taps sort through the core), and a list of rows in compact width; the numbers grid the same. The Mac keeps its NSTableView. |
| 3. The unanchored-popover crash | Done (cause by reasoning and the crash logs) | The two crash logs of slice 5 throw in `UIPopoverPresentationController presentationTransitionWillBegin`, run after a Core Animation commit: the popover was configured with its anchor at `present`, and lost it before its transition began. The parameters inspector dismissed its popover (`n.dismiss(animated: false)`) and presented a new one whenever another node's rows arrived, so two inspector requests in one run-loop turn (the conformance suite's) dismissed a popover whose presentation had not yet run; UIKit clears a dismissed popover's anchor and then ran the cancelled transition. Fixed: the inspector shows another node in the popover already presented (moving its arrow), and `presentReplacing` waits for a presentation still running before replacing it. The guard stays and now says which popover (and whether UIKit made a new controller) if it ever fires. Not reproduced deterministically: the suite's runs since (51 cases, three times on the iPad simulator) did not crash or warn. |
| 4. The banner | Done | Cache Report, the graph's Parameters window and value editors: core models (`report.get`), a window each on the Mac (`CoreWindows`), a sheet on the iPad and iPhone (`PadCoreWindows`), shared SwiftUI (`ReportView`). Compare: `viewer.pin {slot, popout}` from a viewer's bar (Compare › Pin as A / B), `compare.get`, `CompareView` (side by side, split with a divider); the exact difference is not drawn (see decisions). Attach… and Install Engine…: the core's form ask where the shell has no window of its own. `ShellCommands.unbuilt` is empty: nothing a menu offers reaches "isn't available yet". The one remaining text "… isn't available yet in this app" is `Asks.swift`'s for an ask kind the app doesn't know (none today). |
| 5. Drag out; text size | Done | A drag handle in every viewer's bar (both apps) carries the file `value.export` writes, made when the drop asks (`DragOut`). Text size: **removed** from the Apple apps' settings model (`Facts::text_size`, false on the C ABI), since their faces are laid out at fixed platform sizes and honouring it well means scaling the face metrics as well as the fonts; `view.textSize` stays for scripts and egui. |
| 6. iPhone compact layout | Done | Device family 1 and 2. In compact width the bar holds Documents, Add, Run and a More menu (the rest of the toolbar, Settings, the core's menus File, Edit, View, Node, Run, Engine, the engines' state); pop-outs, inspectors, the palette, Settings and the new windows are sheets; tables and grids a list of rows; no Open in New Window where there are no windows. Narrow iPad windows get the same layout through the size class (`--width-class compact` lets a script see it). Conformance 51 of 51 and the UI test on an iPhone 17 Pro simulator. |
| 7. Device script | Done | `claude-debug/native-n4/s4-ipad-device.sh` and a tracked `native/apple/PlumCraftPad/device.sh` (no private defaults: `DEVICE`, `TEAM`) print what the iPad needs first; after launch they fail at once when the address the app reports is on none of the Mac's subnets (`subnet-check.sh`: `ifconfig`'s inet and netmask, tunnels and point-to-point links left out; 10.5.0.2 against the Wi-Fi's /24 fails, the Mac's own address passes). `lan_address` now asks the routing table towards each private block and prefers an address inside the block it was asked about, then 192.168/16, 172.16/12, 10/8 (`choose_lan`, unit-tested on the slice's case). std can't list interfaces or their flags, so a tunnel that is the only address on a private block still wins; no `unsafe`, no new dependency. Not run on the iPad. |

## The floor walk

Each app meets an item **through the core** where the item is the engine interface's (the C ABI core is the same in both apps, and its tests and the conformance suite are the evidence); the UI items cite the command or test.

| F | Item | Mac | iPad and iPhone | Evidence |
|---|---|---|---|---|
| F1 | Engine connection state | Meets | Meets | The bar's engine cell (dot and word) from the feed's `engine` domain; iPad status `engine`; conformance cases on engine state |
| F2 | Reconnect with backoff | Meets | Meets | In the core (`plumcraft-session` links); `crates/session` reconnect tests; same C ABI core in both apps |
| F3 | Liveness / health | Meets | Meets | Core `ping`; `crates/engine` tests |
| F4 | Catalog fetch | Meets | Meets | Core schema fetch; the palette lists every kind (`palette.items`, conformance) |
| F5 | Version gate | Meets | Meets | Core; `crates/proto` hostile sweep and vectors |
| F6 | Engine identity, capabilities | Meets | Meets | Core `caps`; `menu.get` enabled states read them |
| F7 | Catalog hook | Meets | Meets | Core (UI side moved to the core in N1) |
| F8 | NodeDef core | Meets | Meets | Faces from the core's scene; conformance |
| F9 | Wire types | Meets | Meets | Wire drag snaps, rings, Convert dash (N4 slice 1); conformance edit half |
| F10 | Families and icons | Meets | Meets | Node icons (`icon.get`, N4 slice 2); palette by family |
| F11 | Port widget fields | Meets | Meets | Every widget kind edited on the face (N4 slice 2; UI test `testInlineEdits`) |
| F12 | Socket modes | Meets | Meets | Scene rows; conformance |
| F13 | Setting binding | Meets | Meets | Core; inspector sections by group (`parameters.get`) |
| F14 | Variadic inputs | Meets | Meets | Core; scene rows |
| F15 | Choice-dependent sub-ports | Meets | Meets | Core `node.set` replies; inline choice editor |
| F16 | Path hints | Meets | Meets | Path widget with the file ask (N4 slice 2) |
| F17 | External ports | Meets | Meets | Core |
| F18 | Machine-dependent choices | Meets | Meets | Core |
| F19 | Node flags | Meets | Meets | Core |
| F20 | List typing | Meets | Meets | Core; list viewer `items`, `run.item` |
| F21 | Wire slices | Meets | Meets | Slice… is a core command with an in-place name (N4 slice 3) |
| F22 | Custom editors | Partly | Partly | `node.editor` opens a value editor (N4 slice 6): the port's value as JSON, `node.set`; an application's own editor kind (curves, masks) is not drawn |
| F23 | Codegen metadata | Meets | Meets | Core passes `export` through |
| F24 | Expansions | Meets | Meets | Core `node.expand`; menu item |
| F25 | Style graphs | Meets | Meets | Core reads templates |
| F26 | Run request | Meets | Meets | Run button and Command-Return / bar; conformance run cases |
| F27 | Targets | Meets | Meets | Run Selected (`run.selected`, N4 slice 1) |
| F28 | Run identity, generation | Meets | Meets | Core |
| F29 | Latest wins | Meets | Meets | Core |
| F30 | Auto-run | Meets | Meets | Core; Run menu toggle |
| F31 | Optimistic queued | Meets | Meets | Run washes on faces (`run_anim`) |
| F32 | Run reply | Meets | Meets | Core |
| F33 | Per-node report | Meets | Meets | Face status and timing |
| F34 | Errors as data | Meets | Meets | Failed node's face, Messages window (N4 slice 5) |
| F35 | Failure stands in for a reply | Meets | Meets | Core |
| F36 | Cancel | Meets | Meets | Cancel in the bar (iPad More menu in compact) |
| F37 | Run summary for automation | Meets | Meets | Control port `run.*`; conformance |
| F38 | Status record stream | Meets | Meets | Core feed |
| F39 | States emitted | Meets | Meets | Run washes; Colour Nodes While Running |
| F40 | Reducer rules | Meets | Meets | Core |
| F41 | Canvas status display | Meets | Meets | Face header status text |
| F42 | Per-node timing | Meets | Meets | `cached · 0.40 ms` on faces (pictures) |
| F43 | Parameter wires, bundles | Meets | Meets | Core |
| F44 | Provenance | Meets | Meets | Core |
| F45 | Stale tracking | Meets | Meets | Core; faces dim stale |
| F46 | Text on nodes | Meets | Meets | Face excerpts (pictures) |
| F47 | Thumbnails in flight | Meets | Meets | Face previews and waveform thumbs |
| F48 | Texture cache | Meets | Meets | Kit `Pictures` keyed by handle |
| F49 | Inspect piggybacks on runs | Meets | Meets | A watch pops out the value (contract 12.20) |
| F50 | Display payload | Meets | Meets | `popout.get`; every viewer |
| F51 | View modes | Meets | Meets | View As in every viewer bar (`view.viewer`) |
| F52 | Pixel probe | Meets | Meets | Readout and pinned probe (`viewer.probe`, `viewer.data probe`); conformance |
| F53 | Compare | Partly | Partly | Pin A and B from a viewer's bar, side by side or split (`viewer.pin {popout}`, `compare.get`, conformance case); the exact difference is not drawn |
| F54 | Tables and scalars | Meets | Meets | Table viewer sorted and paged by the core; a list of rows in compact width; scalars |
| F55 | Sheet | Meets | Meets | `items` viewer, `viewer.sheet` |
| F56 | Recipe display | Does not | Does not | `viewer.recipe` exists in the core; no Apple viewer shows a recipe yet |
| F57 | Picture window (T16) | Meets | Meets | Pop-out windows (Mac) and sheets or scenes (iPad) |
| F58 | Missing data is a state | Meets | Meets | Viewers say "missing"/"failed" with a reason |
| F59 | Curve editor | Does not | Does not | The research software's editor (N6); not in these apps |
| F60 | Mask editor | Does not | Does not | The research software's editor (N6) |
| F61 | List mapping with axes | Meets | Meets | Core |
| F62 | Vary nodes | Meets | Meets | Core |
| F63 | Lineage | Meets | Meets | Core |
| F64 | Save picture with recipe | Meets | Meets | Save As… on every viewer (`value.saveAs`, the engine writes the recipe) |
| F65 | Export group as plugin | Does not | Does not | `x.vi.export` is the research software's pack; not offered here |
| F66 | Deliver plugin | Does not | Does not | As F65 |
| F67 | Plugin round trip | Meets | Meets | Core expansions |
| F68 | Arrays never in messages | Meets | Meets | Values lent by handle (`plum_blob`) |
| F69 | Content addressing | Meets | Meets | Core, b3 |
| F70 | Hostile `.npy` and keys | Meets | Meets | `crates/*/tests/hostile.rs` and `crates/media/tests/hostile.rs` |
| F71 | Engine memo | Meets | Meets | Core; Cache Report (N4 slice 6) |
| F72 | Partial re-run | Meets | Meets | Core |
| F73 | Preview latency path | Meets | Meets | In-process engines over the C ABI |
| F74 | Full resolution on demand | Meets | Meets | `viewer.zoom detail` |
| F75 | Pictures as engine-side files | Meets | Meets | Core |
| F76 | Workflow JSON is the payload | Meets | Meets | Documents are workflow JSON |
| F77 | Recipes in pictures | Meets | Meets | Engine writes them on save |
| F78 | Bundles | Meets | Not walked | Mac: File › Export Bundle… (the file ask); the iPad's and iPhone's path (the core menus, a save ask answered in the app's folder) was not exercised by a test this slice |
| F79 | Insert graph or group | Meets | Meets | File › Insert… (core menus; iPhone via More) |
| F80 | Engine-touching commands | Meets | Meets | `enabled` from the core; Attach… form (N4 slice 6) |
| F81 | Control channel | Meets | Meets | The control port on both apps; conformance runs over it |
| F82 | Headless CLI catalog | Meets | Meets | Core / CLI, not the apps |
| F83 | Config by environment | Meets | Meets | Core config and launch flags |
| F84 | Single-threaded engines | Meets | Meets | Core |
| F85 | Local model management | Does not | Does not | The research software's CLI; not an app feature |

Summary: 77 met on the Mac, 76 on the iPad and iPhone; partly: F22 (custom editors: a JSON value editor, not an application's own kind) and F53 (Compare: no exact difference); not met: F56 (recipe display: the core has `viewer.recipe`, no Apple viewer shows it), F59 and F60 (the research software's curve and mask editors, stage N6), F65 and F66 (plugin export and deliver: the research software's pack), F85 (a CLI command); not walked: F78 on the iPad and iPhone. The small gaps fixed this slice: F53 partly, F22 partly, F80 (Attach…). Left: F56 (a Recipe button on the viewer bar over `viewer.recipe` is small, next slice), Compare's difference.

## N4's gate (note 24 section 12)

| Part | Met? | Evidence |
|---|---|---|
| The whole suite | Met in the simulators and on the Mac | 51 cases, 0 skipped, on the Mac app, the iPad simulator, the iPhone simulator, the C ABI headless and over its port, the egui editor, the core's own document; the Swift kit's tests |
| The suite on the device | Not measured | `device.sh` takes it; not run (instructed) |
| The floor's list walked on each | Walked | Table above |
| A notarized Mac build | Not done (Apple) | An unsigned Release archive builds (`s6-build/archive/PlumCraft.xcarchive`); see what the upload needs |
| A TestFlight build | Not done (Apple) | Device builds Debug and Release (unsigned) build clean |

## What the Apple upload still needs

| Need | State |
|---|---|
| App icon set | Missing in both apps (no asset catalog): a design decision |
| Privacy manifest | Draft at `claude-debug/native-n4/s6-release/PrivacyInfo.xcprivacy` (no tracking, no data collected; UserDefaults CA92.1, file timestamps C617.1, disk space E174.1, boot time 35F9.1): to confirm against the binaries and add to both targets |
| Usage descriptions | Release: none needed today (no camera, microphone, photos, location); the local-network text is in Debug only, checked: absent from the Release device build's Info.plist, present in Debug's |
| Encryption declaration | `ITSAppUsesNonExemptEncryption` not set: the apps use hashing (b3) and no encryption of their own that I found; the maintainer to decide and set `NO` if so |
| Version and build numbers | 0.7.0 (1) in both `project.yml`; a build number scheme for uploads is needed |
| Entitlements and signing (Mac) | `ENABLE_HARDENED_RUNTIME: NO` today; notarization needs it on, with Developer ID signing; engines started as processes may need entitlements under the hardened runtime (not tested); export options draft `s6-release/ExportOptions-mac-developer-id.plist` |
| Signing (iOS) | Distribution identity and an App Store profile; export options draft `s6-release/ExportOptions-ios-testflight.plist` (export only, no upload) |
| The debug-only network port | Absent from Release: the C ABI refuses a non-loopback bind without `network_debug` and a pre-shared token, which only `#if DEBUG` sets; the Release Info.plist has no local-network text (checked) |
| An App Store Connect record, TestFlight testers, review notes | The maintainer's |

## Core, C ABI, kit and contract changes

- **plumcraft-editor-core:** B41 (`settings::after_command`, `Look::keep_run_pref`, `RunPref`, `get {fromDisk}`), `prefs::Store::{set_run_colors, set_anim_ms}`; `reports.rs` (new: `report.get`, the Cache Report, Parameters and value editor models; `format_bytes`, `tree_text`, `refusals`); `windows.rs` (`Windows.{cache, params, editors, compare, reports}`, `compare_json`); `control.rs` (`report.get`, `compare.get`); `invoke.rs` (`engine_form`: Attach… and Install Engine… as the core's form where the shell has none); `Facts::text_size`.
- **plumcraft-session:** `viewer.pin {popout}`, enabled when a pop-out has a value.
- **plumcraft-automation:** `lan_address` probes each private block; `choose_lan` and its test.
- **plumcraft-ui (egui):** offers the text size (`Facts::text_size: true`); nothing else.
- **plumcraft-ffi:** a test (`attach_asks_with_the_cores_form_and_install_is_refused_here`); no code change. **`PLUM_ABI_VERSION` unchanged (1).**
- **Kit:** `Windows` fields and `reportKeys`; `PlumCraftShell/Reports.swift` (new: `ReportModel`, `ReportView`, `CompareModel`, `CompareView`, `CoreWindowKeys`); viewer bar Drag Out and Compare; `PageGrid` on iOS; `ShellCommands.unbuilt` empty; Settings' Done on every page; the palette's opaque ground.
- **Apps:** Mac `CoreWindows`; iPad `PadCoreWindows`, compact bar and More menu, `--width-class`, opaque sheets, the inspector's reuse, `presentReplacing`'s wait, device family 1 and 2, `device.sh`, `subnet-check.sh`.
- **Contract:** `docs/control-protocol.md` (`report.get`, `compare.get`); `docs/shell-interface.md` 12.5 as built in slice 6; `schema/shell1.json` regenerated last.
- **Conformance:** two new cases (51): the Cache Report and Parameters window opened, read, edited by their commands and closed; Compare pinned from two pop-outs opened by a person's double tap.

## Conformance

| Driver | Result |
|---|---|
| The core's own document, the C ABI headless and over its port, the egui editor (Rust) | pass (7 suite tests) |
| The Swift kit (`swift test`) | 81 tests pass |
| The Mac app (`conformance.sh all`) | 51 of 51, 0 skipped |
| The iPad app in the simulator | 51 of 51, 0 skipped (the last build, after every fix) |
| The iPhone app in the simulator (iPhone 17 Pro, iOS 26.5) | 51 of 51, 0 skipped |
| The iPad UI tests (`conformance.sh gestures`) | 5 of 5 (`testCompactWindowsByATap` new: More › View › Parameters…, Settings › Cache › Open Cache Report, Engine › Attach… and its form, a value's Compare › Pin as A, each by taps, with `--width-class compact`) |
| The iPhone UI test (`testCompactWindowsByATap`) | passed (iPhone 17 Pro). The iPhone's conformance (51 of 51) ran on the build before the last two iPad fixes (value sheets dismissed from their presenter, Settings' Done on every page); its UI test ran on the last build |

The first iPad run of the new Compare case failed: the iPad shows one value's sheet at a time, so opening the second value closed the first in the core; the case now closes the first before opening the second (a case written for the Mac's windows, not a product defect).

## Tests

- `cargo nextest run --all-features --no-fail-fast`: 1,535 of 1,537 (17 skipped); the failures: the known local-only `params_coverage::hidden_params_match_the_baseline`, and `shell_schema::checked_in_schema_is_current`, stale until the schema was regenerated last (as instructed). None of the known intermittents failed. New Rust tests: B41's, `the_local_network_is_chosen_over_a_vpn_tunnel`, `reports` (two), the FFI Attach form.
- `cargo clippy --all-targets --all-features -- -D warnings` clean; `cargo fmt --all --check` clean (fmt run before clippy and nextest).
- `scripts/check-hash-gates.sh`, `scripts/check-core-toolkit-free.sh`: pass.
- Python: 224 passed; ruff clean.
- Builds: xcframework; Mac Release with no warning; iPad/iPhone simulator Debug and Release with no warning; device arm64 Debug and Release unsigned (Debug prints Xcode's five "Conditional compilation flags do not have values" notices for `_LIBCPP_HARDENING_MODE` on the kit's package targets, a build-setting notice, as in slice 5); an unsigned Mac Release archive.

## Store and preferences

Every script sources `claude-debug/native-n4/s6-env.sh` (which refuses an unset store or one under `/Volumes/<store>`) and checks again itself; the Python stages check too. `s6-store-before.txt` / `-after.txt`, `s6-prefs-before.txt` / `-after.txt`. **Preferences and trusted files:** unchanged (`prefs.json`, `trusted.json`, `com.plumcraft.mac.plist` checksums identical before and after). **The real store:** its top-level listing is identical before and after; nothing in it was modified on 2026-11-21 (the after listing ran past midnight, so its "modified today" part is empty, where the before listing's named entries modified on 2026-11-20 before this slice began, all `.hello`, `.usage.json`, `.writable.*`, `plummemo/ab`, `plumz`, the maintainer's running apps' own). No process of this slice pointed at it (every launch had a scratch root; the iPad and iPhone apps keep theirs in their containers, checked by `conformance.sh`).

## Processes and simulator

- `pgrep -lx` at the end: `5861 PlumCraft`, `30260 PlumCraft`, `54951 PlumCraft`, `58303 PlumCraft` (the maintainer's, untouched); no `PlumCraftPad`, `CanvasSpike`, `CanvasSpikeMac` or `xctrace`. None is mine.
- Simulators: the "iPad Pro 13-inch (M5)" left booted, `com.plumcraft.pad` and the UI test runner uninstalled (only `com.plumcraft.spike.canvas`, not mine, remains); the "iPhone 17 Pro" (`EB0C56AD-…`) booted by my scripts for each round and shut down by them after, shut down now; nothing erased. The physical iPad was not used.
- Build folders: `claude-debug/native-n4/s6-build/mac` (the Mac Release app, left for the maintainer: `s6-build/mac/Build/Products/Release/PlumCraft.app`), `s6-build/archive/PlumCraft.xcarchive` (52 MB, the unsigned archive, left as evidence); `s6-build/device` and the archive's derived data removed. Disk: 78 GB free at the end.

## Pictures

`claude-debug/native-n4/s6-shots/`: iPad (`pad-*-light.png`, `pad-*-dark.png`): canvas, table, table sorted, JSON as YAML, picture, array grid, Compare side by side and split, audio, Cache Report, Parameters, Settings, palette; the iPad at a narrow width (`pad-narrow-*`); iPhone (`phone-*-light/dark.png`): the same set (canvas, palette, a pop-out, the table's list form, settings among them); the UI tests' `n4s6-ui-*` (compact canvas, Parameters, Cache Report, the Attach form, the table in compact, Compare) for the iPad and, under `phone-ui/`, the iPhone. Mac: none (see the first section); `mac-report-*.json` say what opened.

## Defects introduced and caught

- The look locked twice in `after_command` (B41's first fix): a hang, caught by the FFI test.
- Compare's pictures were blank: a rendition's handle is under its DataRef (`ref.handle`).
- The report windows didn't rewrite the status file, so scripts timed out waiting (the windows were there).
- A core sheet dismissed by another presentation stayed open in the core, so asking again showed nothing: now closed in the core when replaced or when what it was over goes.
- The iPad's Settings sheet had Done only on its first page.
- The Mac's status kept a closed report window until the next change.
- A value's sheet closed by the core with Compare stacked over it stayed on screen ("Reading the value…" under Compare): UIKit's `dismiss` on a controller that presents another dismisses only the child. Value sheets are now dismissed from their presenter.
- An optional chain on a non-optional host (a compile error, caught by the build before any run).

## Known and left

- **A scripted race on the iPad:** when a script opens a value, pins it, opens another while Compare is shown and closes it, all in a few hundred milliseconds, the second value's sheet can stay under Compare ("Reading the value…" in `pad-compare-*.png`). The person's path (the UI test) doesn't hit it. Presentation changes from the core arriving faster than UIKit's transitions want a queue in `GraphViewController` (one presentation change at a time); not built.
- **Compare's exact difference** (decision 2), **the recipe display** (F56), **value editors of an application's own kind**, **the suite on the device**, **the notarized and TestFlight builds**.
- The palette's popover (regular width) is still drawn on the system's glass; only its compact sheet has the opaque ground.

## Decisions for the maintainer

1. **Text size removed from the Apple apps' Settings** rather than half-honoured (the faces would need scaled metrics, not just fonts). Keep removed, or build it properly later.
2. **Compare without the exact difference** in the Apple apps (side by side and split only; the note says the difference is egui's). The difference needs the full arrays read on the media service's workers (a new derived kind, as the spectrogram is): a slice of its own.
3. **Value editors draw the value as JSON** whatever the kind: an application's own editor (curves, masks) stays the research software's to write in Swift (N6).
4. **On the iPad, a report window or Compare is a sheet stacked over what is shown** (Settings, a value's sheet), not a replacement; keep, or one sheet at a time.
5. **The iPhone runs the iPad app's target** (one app, `com.plumcraft.pad`, device family 1 and 2) rather than a separate iPhone target; keep, and decide the bundle id for the store.
6. **The upload items above** (icon, privacy manifest, encryption flag, hardened runtime and entitlements, build numbers).

## What only a person can verify

- **Mac:** View › Parameters… (rename a parameter in its Name field and press Return; Move Up, Go To, Remove); Settings › Cache › Open Cache Report; a value's bar: Compare › Pin as A, open another, Pin as B, the Compare window, Split and drag its divider; drag the four-arrow handle from a viewer's bar to the Finder and to Mail; Engine › Attach… (the form; a wrong address keeps it open with why).
- **iPad:** the same through the menu bar; a value's sheet in dark is opaque (no graph through it); the table starts at the left edge, a header tap sorts and again reverses; Split View and Slide Over at a narrow width: the compact bar's More menu, the table as a list; the parameters inspector opened quickly on two nodes in a row (no crash).
- **iPhone:** More › View › Parameters…, More › Settings › Cache › Open Cache Report, More › Engine › Attach…; double-tap a pin: the value's sheet, its bar's Compare; the palette from +; rotate to landscape.

## What N5 (Android) should know

- Everything a window shows here is a core model (`report.get`, `compare.get`, `settings.get`, `popout.get`, `viewer.data`) and every control a command: draw them, don't compute them.
- iPadOS 26 made the system grounds see-through in partial sheets; expect the platform's own surface rules (Material 3 bottom sheets) to need an explicit ground too.
- Compact width needs a menu for the core's menu bar (the iPhone has none) and a list form for tables.
- A run setting changed in one document must write only that setting (B41); the core does it now, so nothing to do in the shell.
- `lan_address` can't tell a tunnel from the network without interface enumeration; on Android `NetworkInterface` can, so check the device's address there.
