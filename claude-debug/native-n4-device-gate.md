# N4's device gate: what is ready for the iPad

Branch `native/n4-device-gate`, made from `feat/native-n0` at `5f0cb5f`. Nothing is committed. The physical iPad and iPhone were not touched. Nothing went to Apple.

## Answer first

- **B42 is fixed at its source and verified off the device.** The C ABI core now writes exports, copies and scratch files under the folders the shell gives it. A Rust test runs with `HOME` read-only. In the iPad simulator, with the data container's root made read-only, slice 6's build fails the same step the iPad failed. This branch passes all 52 cases there.
- **B43 was reproduced in the simulator and fixed.** The iPad shell closed a core window by itself: it presented a sheet over the Messages sheet while that one was still going away. In the probe this happened in 6 of 20 rounds before the fix and 0 of 20 after.
- **The UI tests can now read the app's status on a device**, through an accessibility element. They also failed on any second run in the same container, because autosave wrote each test's edits into the sample. That is fixed too (B44).
- **The 500-node hitches are not a regression of slices 4 to 6.** In one simulator, every build from slice 3 to now draws exactly the same faces. On the device, the low slice 3 numbers came from a run with about 40% less face raster on screen. N3's build, measured with the same amount on screen as slice 6, hitched more than slice 6 did. I made no fix for the hitches: their real cost has to be measured on the device.
- **The device pass is one command:** `claude-debug/native-n4/s7-device-pass.sh` (about 45 minutes).

## B42: exporting fails on a physical iPad

**Cause.** `plumcraft-ffi`'s `Config` read `dirs.prefs` and `dirs.logs` but never read `dirs.caches` or `dirs.scratch`. Each document's `DocApps.root` (copies opened in apps, and `value.export`'s folder) therefore stayed at `doc_cmds::default_root()`. On iOS that is `HOME/.cache/plumcraft/open`: `cfg!(target_os = "macos")` is false on iOS, so the desktop XDG rule applied. On iOS, `HOME` is the container's root, which a device app may not write; the simulator allows it.

**Every place audited** (core and crates, non-test code):

| Path | Before | Now |
|---|---|---|
| Open-in-app copies and exports (`DocApps.root`) | `HOME/.cache/plumcraft/open` on iOS | `dirs.caches/open` when the shell gives it; iOS default `Library/Caches/plumcraft/open` |
| Autosave's `scratch: name` | `plumcraft_scratch` (the process `TMPDIR`) | `dirs.scratch/autosave` when given |
| Craft root default (`plumcraft-craft` `pool::default_root`) | `HOME/.cache` on iOS | `Library/Caches` on iOS (the iPad also sets `PLUMCRAFT_CRAFT_ROOT`) |
| HTTP host CAS (`plumcraft-host` `default_cas`) | `HOME/.cache` on iOS | `Library/Caches` on iOS |
| Preferences (`prefs::path_for`) | `HOME/.config` on iOS | `Library/Application Support` on iOS (the iPad also sets `PLUMCRAFT_EDITOR_PREFS`) |
| Store, marks | from the environment the iPad sets | unchanged |
| Log file | `dirs.logs` | unchanged |
| Default screenshot path, `plumcraft_scratch` root, control token default | `TMPDIR` (the container's `tmp`, writable on a device) | unchanged |
| Subprocess and video work folders (`host::process`, `array::channels::video`) | `TMPDIR` | unchanged: they don't run on iOS |

**Found on the way, also fixed.** A short text value comes in its excerpt, with neither `value` nor `full`. It was exported and copied as an empty file. Test: `export::tests::a_short_text_carried_in_its_excerpt_is_taken_out_whole`.

**Tests**

- `crates/ffi/tests/dirs.rs`: the test binary runs itself again with `HOME` a read-only folder and every `dirs` root writable. It covers export (the same file Drag Out uses), `value.share`, `graph.share`, autosave into scratch and into the document's own file, and the open-value cache's root (`ui.inspect`'s `docs.root`). It also asserts that nothing was written under `HOME`. With the one line that sets `docs.root` reverted, it fails: "copies for apps go to …/home/Library/Caches/plumcraft/open".
- `claude-debug/native-n4/s7-b42-sim.sh`: the iPad simulator with the data container's root `chmod 555`. Slice 6's Release build failed step 13 of the array case, as the iPad did ("…/.cache/plumcraft/open/export…"). This branch passed 52 of 52 with its Debug build, and again in the final Apple stage.

**Not made to fail off the device.** Save As goes through the engine to the path the shell's `file.save` ask returns (Documents on the iPad). The compiled-in engines save no formats, so Save As could not be exercised at all. The read-only-root simulator test reproduces the device's own sandbox rule for the container's root only; any other device-only rule is left to the device.

## B43: Parameters not reported open on the device

**Cause.** The iPad shell's. In the suite, the Messages case runs just before this one. Its sheet is presented with animation, and the case closes it at once: the dismissal was asked for while the presentation was still running. The Cache Report was then presented over the Messages sheet, and Parameters over the Cache Report. When the Messages sheet finally went, it took both with it. `PadCoreWindows.follow` saw its sheet with no presenter and told the core it was closed.

**Reproduced.** `s7-b43-probe.py --messages`, in the simulator with the app launched exactly as `device.sh` launches it, closed Parameters by itself 0.11 s after it opened in 6 of 20 rounds. The case alone, run 30 times, never failed, which is why it passed everywhere but in the suite on the slower device.

**Fix.** `Presentations.whenSettled` (in `GraphViewController.swift`) is one ordered queue. Every presentation change the core drives goes through it, one at a time, once nothing presented is mid-transition: the core windows, Messages, Settings, a value's sheet replaced or closed by the core, and `presentReplacing`. A core window now counts as closed by something else only after its own presentation has run. After the fix: probe 0 of 20, the case 30 of 30.

**A new case and a new step form.** The case is "windows opened as another closes stay open: the shell never closes what the core opened". The step form is `holds` (with `for`): a condition that must stay so for a time. Both runners (Rust and Swift) take it, and `docs/shell-interface.md` says so. The new case passed once on slice 6's build, so it catches the defect only some of the time.

## The UI tests on a device

**Choice: an accessibility element.** `--status-element` is in debug builds only. The app puts its status JSON in the accessibility value of a 2-point view, in a window of its own above every sheet. `GestureTests` reads it with `descendants(matching: .any)["plumcraft-status"]`. It does so on a device by default (`#if targetEnvironment(simulator)`), or wherever `PLUMCRAFT_STATUS_VIA=element` says.

Why this way:

- It needs no network, entitlement or provisioning change.
- An app group would need a registered group, and so `-allowProvisioningUpdates`.
- The control port would need the shell's status exposed through a new contract method.
- The simulator keeps the file.

**Found: the UI tests failed on any second run in the same container (B44, fixed).** Every document autosaves, and `--open-sample` opened the copy in Documents/Samples. So `testInlineEdits` and `testWindowsAndNames` started from the previous run's edits, in either reading mode. On a device the container stays, so every run after the first would fail. With `--scratch-state`, the sample now opens as a fresh copy in scratch.

**Results in the iPad simulator.** File mode: 5 of 5. Element mode on a fresh install: the two tests that had failed now pass. Three runs in one container (file, element, file): see "Tests and counts" below.

**`device.sh`**

- It prints Auto-Lock Never, power and Settings > Developer > Enable UI Automation among the iPad's needs.
- Each run gets its own status file and its own result bundle.
- It stops with exit 5 on a launch refused because the iPad is locked, and says so.
- It recognises "Authentication canceled".

## The hitches at 500 nodes

**(a) Reading the code.** Between slice 3 (`5e6d9ec`) and slice 6 (`9b90500`), the bench's drawing path changed in only four places. Slice 4 added a Reduce Motion check on a running face and appearance observers. Slice 4 also made numbers use the core's text, which is the same text as before for the bench graph. Slice 5 added the waveform preview, which the bench graph has none of. Nothing changed layer counts, shadows (they are baked into each face's picture), masks or raster scale. The bench runs in its own view controller, so the iPad's new bar, sheets and menus are not on screen during it.

**(b) Measured.** The 500-node bench in the iPad simulator, on all five Release builds, twice each:

| Build | Face pictures (MB) | Hidden while panning | Hidden in the fit | Hidden while zooming | Late frames |
|---|---|---|---|---|---|
| Slice 3 (`5e6d9ec`) | 173.7 | 200 | 0 | 446 | 0 |
| Slice 4 (`a57b29a`) | 173.7 | 200 | 0 | 446 | 0 |
| Slice 5 (`2718e92`) | 173.7 | 200 | 0 | 446 | 0 |
| Slice 6 (`9b90500`) | 173.7 | 200 | 0 | 446 | 0 |
| This branch | 173.7 | 200 | 0 | 446 | 0 |

Every build draws the same faces at the same scales and culls the same layers. On the device, the 500-node bench JSONs fall into two groups:

| Run | Build | Face pictures (MB) | Hidden while panning | Pan, fit, zoom, drag (ms of hitch a second) |
|---|---|---|---|---|
| s5, 2026-11-20 13:17 | N3 | 122.8 | 180 | 136.7, 341.4, 116.8, 30.3 |
| s5b, 21:34 | after slice 4 merged | 120.9 | 180 | (no trace) |
| s5c, 21:57 to 22:00 | the "slice 3" comparison | 73.1 | 413 | 10.4, 22.9, 3.5, 29.2 |
| s6d to s6i, 2026-11-21 | slice 6 | 120.9 | 160 to 180 | 117.8, 168.9, 84.0, 11.5 |

Only s5c had 40% less face raster and 2.5 times as many faces culled. That is a different amount of graph on screen: the canvas's size and orientation set both the fitted zoom, and with it the raster scale, and how many faces are culled. s5b and s5c ran 23 minutes apart without a rebuild.

**Conclusion.** Not a code regression; the slice 3 baseline was taken in another configuration. Confidence: fairly high. The simulator shows every build identical, and the device groups match the configuration, not the build. Only the device can confirm it, by the bisect below at one fixed orientation.

**What the hitches are.** They are real and older than N4: N3's build had them in the same configuration. Their reasons are "Potentially expensive render / GPU work", with up to 68 offscreen passes a hitch, and the app's own late-frame count stays at 0. So the cost is on the render server's side. The N0 spike's control showed 0 hitches at 500 nodes in the one control trace that recorded phases (s5, N3 day).

Two hypotheses for a fix, to test on the device:

- The 800 wire shape layers.
- The 120 to 170 MB of face textures sampled every frame.

The bench now records its view size, orientation, each phase's zoom and raster scale and the face pictures' size. It can also draw no wires (`--hide-wires`), to separate the wires' share from the faces'.

**(c) The device bisect** is part of the one command. On each prebuilt Release build (slices 3, 4, 5, 6 and now), it does:

- one bench run, which checks the rate and stops at 60 Hz;
- two 500-node hitch traces.

Around those, it records:

- the spike's control trace, once a session;
- "now" with no wires drawn;
- "now" after the iPad is turned, if the person agrees.

It prints one table. The builds are in `claude-debug/native-n4/s7-builds/<name>/{sim,device}`; the device apps are signed with the team's existing development identity. Each old commit's source is a detached git worktree, `s7-builds/wt-<name>`, which shares this checkout's `target/`.

## Bench script flaws (item 5)

These are fixed in the tracked `native/apple/PlumCraftPad/bench.sh`:

- Each run's JSON has its own name (`bench-N-<time>-<pid>.json`).
- The app removes earlier `bench-*.json` from Documents when a bench starts.
- A result in `OUT` under the name about to be written is removed first.
- A launch refused because the iPad is locked stops with exit 5 and reads nothing.
- A copy that finds no file is a failure, not success.
- A trace with no phases is not a result.
- `APP=` runs a prebuilt app.
- The hitch binner, `hitches.py`, is now beside the script rather than in `claude-debug`.

## The other two items

- **Testing kinds hidden (card 31 B).** `plumcraft_editor_core::palette::offered` leaves out any kind with `Testing` in its category path. It applies to the palette index, so to `palette.items` and the egui palette, and to the Node › Add menu. `node.add` by id still adds such kinds. No conformance case added them through the palette. One egui test looked for a Testing kind in the palette; it now looks for Upper case and asserts that Knobs is absent. Tests: `palette::tests::testing_kinds_are_not_offered_but_are_added_by_id`, `menu::tests::node_add_offers_no_testing_kind`. The contract text is in `docs/shell-interface.md` and `docs/control-protocol.md`.
- **`Gesture.zoom` in the shell schema.** It was added to `shell_schema::Gesture` and to the contract's `ui.gesture` line, and `schema/shell1.json` was regenerated last. The Swift `Gesture` model already had `zoom`. The schema test passes, and the Swift kit's tests pass (81). `native/windows/` has a kit test that sends a drag without `zoom`, with a comment saying why. It was not touched; the controller will have it updated.

## Tests and counts

| What | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --all-features --no-fail-fast` | 1,540 of 1,542 (17 skipped): the known `params_coverage::hidden_params_match_the_baseline`, and `shell_schema::checked_in_schema_is_current` until the schema was regenerated last (then passed) |
| `scripts/check-hash-gates.sh`, `scripts/check-core-toolkit-free.sh` | pass |
| Python | 224 passed, ruff clean |
| `swift test` (kit) | 81 tests pass |
| Rust conformance drivers | 7 suite tests pass |
| Mac app conformance | 52 of 52 |
| iPad simulator conformance | 52 of 52 |
| iPhone 17 Pro simulator conformance | 52 of 52 |
| iPad simulator, container root read-only | 52 of 52 (slice 6's build: 51, the export step failed) |
| iPad UI tests (file) | 5 of 5 |
| iPhone UI test | passed |
| Builds | xcframework; Mac Release; iPhone simulator Debug and Release; device Debug and Release unsigned, with no warning beyond the five `_LIBCPP_HARDENING_MODE` notices as before; signed device Release apps for the five builds; the local-network text is in Debug and absent from Release |

**The UI tests again, after B44's fix.** The five tests ran three times in one iPad simulator container, with no reinstall between runs: file mode, element mode, file mode. Each run passed 5 of 5. Before the fix, a second run failed two of them in either mode.

**The Mac app again, after its scratch folders change.** Conformance 52 of 52. Nothing was written to `~/Library/Caches/com.plumcraft.mac` or `~/Library/Logs/PlumCraft`.

**The bench in the simulator with `--hide-wires`.** It ran. It recorded the view (1032 × 1376 points, portrait), the fitted zoom (0.157) and the raster scale (0.5).

## Defects I introduced and caught

- The first `s7-b43-loop.sh` hit an unbound empty array under `set -u` (bash 3.2) and exited before running. The same mistake in `s7-ui-again.sh` was caught and the run stopped by its task before the second pass.
- The first version of the B42 test expected Save As to work through the text engine, which saves no formats.
- Rebuilding the "now" apps with `REBUILD_XCF=1` left the main checkout's xcframework without its macOS slice. It was rebuilt in full before any Mac stage.
- The Mac app's scripted runs wrote their exports into the person's `~/Library/Caches/com.plumcraft.mac/open` once the core took `dirs.caches`. Before this branch they went to `~/Library/Caches/plumcraft/open`, also outside scratch. My one export there was removed. The Mac app now keeps a scripted launch's caches and log beside its scratch marks; see the Mac rerun below.

## Store, preferences, processes, simulators

- **The real store.** Every stage sourced `claude-debug/native-n4/s6-env.sh`, which refuses an unset store or one under `/Volumes/<store>`; the s7 scripts and the Python probes check again themselves. The before and after listings (`s7-store-before.txt`, `s7-store-after.txt`) are identical, and nothing in the store was modified today in either.
- **Preferences.** `prefs.json`, `trusted.json` and `com.plumcraft.mac.plist` have the same checksums before and after.
- **Writes outside scratch:**
  - the source tree and its `target/`;
  - the iPad project's `build/` and `build-device` (allowed);
  - four registered git worktrees under `claude-debug/native-n4/s7-builds/`;
  - one export in `~/Library/Caches/com.plumcraft.mac/open`, since removed;
  - one append to `~/Library/Logs/PlumCraft/plumcraft.log` at 11:33, by the first Mac conformance run.

  The last two led to the Mac app's scratch folders change.
- **Processes.** `pgrep -lx PlumCraft` at the end shows 5861, 30260, 54951, 58303 and 92471: the maintainer's, never signalled. No `PlumCraftPad`, `xcodebuild` or `xctrace` of mine is left running. The one bash task I stopped had been started by me.
- **Simulators.** The iPad Pro 13-inch (M5) is booted, as it was. `com.plumcraft.pad` and the UI test runner are uninstalled; only `com.plumcraft.spike.canvas`, which is not mine, remains. The iPhone 17 Pro was booted by the Apple stage and shut down by it. Nothing was erased.
- **Disk.** 63 GB free at the end, 37 GB at the lowest. No snapshots were thinned.

## What only the device can verify

- The suite on the iPad: B42's export step and B43's case.
- The UI tests through the status element on the device, and the element's speed there.
- Whether the hitches match across builds at one orientation (the bisect).
- Whether turning the iPad reproduces the s5c numbers.
- What the wires cost (`--hide-wires`).

## The one command

```sh
claude-debug/native-n4/s7-device-pass.sh            # all: suite and UI tests, budgets, bisect
claude-debug/native-n4/s7-device-pass.sh table DIR  # the table again from a pass's folder
```

It prints the iPad's needs first:

- unlocked, on power, Auto-Lock Never;
- landscape, full screen, not turned;
- Low Power Mode and Limit Frame Rate off;
- the same network, VPN off;
- Enable UI Automation on;
- someone to answer the local-network question.

| Part | Time |
|---|---|
| Debug build and the suite | 5 to 8 min, the suite about 40 s |
| UI tests | 8 to 12 min |
| Budgets | about 4 min |
| Bisect | about 20 min |
| The iPad turned, if agreed | about 3 min |

It stops with exit 5 if the iPad is locked and exit 3 at 60 Hz, and writes everything to a new `claude-debug/native-n4/s7-device-<date-time>/`.

## Decisions for the maintainer

1. **The UI tests' status on a device through an accessibility element** (debug builds only), rather than the control port or an app group. Keep it, or move the shell's status behind a control method later.
2. **Scripted launches (`--scratch-state`) open samples as fresh copies in scratch** and keep the Mac app's caches and log in scratch. Keep it.
3. **The hitches:** treat them as N3's standing cost at 500 nodes rather than an N4 regression, and decide the fix from the device pass. If wires dominate, look at the wire layers; if faces dominate, look at their raster sizes when zoomed out.
4. **The old-commit worktrees** `claude-debug/native-n4/s7-builds/wt-s3` to `wt-s6` are registered with git. Remove them (`git worktree remove`) after the device pass.
