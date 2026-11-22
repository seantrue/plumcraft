# The decided batch after the person check (2026-11-21, branch `native/n4-decided-2`)

From `origin/fix/manifest-warn-2` (merged into `feat/native-n0` as #156). Nothing committed. The maintainer's answer: "1-3 yes, 4 - background, 5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20 yes. Do 19,20 ASAP", and after the stray save, "1,2 as you say". Both are recorded in note 24.

## What was built

| Item | Decision | State | What verified it |
|---|---|---|---|
| 1 | 7 (B84), 9 (B81) | Done, made safe | Person check in the Mac gate: the person's build refuses `--scratch-state`; the save and open panels open on a scratch folder, read from the panel itself, cancelled; Open Recent from the copy's own list; Library, home listings and engines folder unchanged; the scripted id's Library cleaned. `may_press`: 12 of 12 |
| 2 | 8 (B83) | Done | Server, C ABI port and egui tests; the engine half reads its own window by `doc` |
| 3 | 11 (B86, B87) | Done | Core and C ABI tests; the person check's Run on and launch banner steps |
| 4 | 13 | Done | `palette::tests::an_exact_title_comes_first`; a conformance step (core, C ABI, egui, Mac) |
| 5 | 12 (B91, core half) | Done | `a_hosting_whose_program_is_missing_is_refused_with_the_reason` (bundle with and without the program, a cargo layout); the full test run with the programs in the profile folder |
| 6 | 17 | Done | Core test; the person check: the Mac's Engine menu item's image and tooltip, the engine cell's mark once in use. iPad: built, not launched with a warned engine |
| 7 | 16 (Swift half) | Done | Swift tests: the copied-manifest steps through the kit (and failing on the suite's folder), the two matchers; the Mac conformance claims `copied_manifest` (57 of 57) |
| 8 | 18 | Done | Rust and Python register tests |
| 9 | 15 (B56) | Done | Core, C ABI, egui conformance; Mac 57 of 57 |
| 10 | B76 | Done; the device half unverified | A Mac recording: the raw trace is DTServiceHub's, in the per-user temporary folder; the sweep removes it, none left |
| 11 | 10 (engine half) | Eleven rows newly met, and more of F39 | The person check's engine half on the launched Mac app; the Mac table 43 met, 2 partly, 40 not met (from 32, 2, 51) |

## The proof for item 1

Every person check and conformance run since the safe version lists, before and after: the person's home top level (names, 567 lines), `~/workspace` top level (names), `~/Documents` and `~/Desktop` (name, size, time), and the person's Library for `com.swapwizard.plummer.mac` and `com.swapwizard.plumcraft` (defaults as cfprefsd answers and the plist's size, time and hash, saved state, the recent-documents `.sfl4` with its hash, caches, logs, application support, HTTP storage, WebKit, cookies). In every run from the second on, both listings were the same after as before ("HOME nothing came or went…", "LIBRARY the person's Library is unchanged"); the person's `com.swapwizard.plummer.mac.sfl4` kept its size, time and hash. The scripted id's Library before the run was empty (cleaned); after the run it held the panels' keys (`NSOSPLastRootDirectory`, `NSNavPanelExpandedSizeForOpenMode`) in `com.swapwizard.plummer.mac.scripted.plist` and its own `.sfl4`; after the cleanup it was the same as before. The listings of one run are kept untracked in `claude-debug/n4-decided-2/person7-proof/` (they name the person's files, so they are not tracked).

The first run (before the safe version) is the exception: its home listing showed `~/Documents/Untitled.json` (640 bytes) new; that run is why nothing is pressed now.

## Contract changes

- `docs/control-protocol.md`: a request's top-level `doc` (honoured on the C ABI's port, `unknown_doc` and `bad_params`; refused by egui); `palette.items`' order.
- `docs/shell-interface.md` 4.1 (the port's `doc`), 4.2 (a hosting whose program is missing is disabled with the reason; what Windows must ship), 12.x `palette.items`' order.
- `docs/interface.md` 16.6: `engine.manifestProblems`, the start's status line; `register --print`'s warning.
- Additive fields: `engine.list`'s `manifestProblems`; `ui.inspect`'s `engine.manifestProblems` and `engine.warning`; the C ABI feed's engine change `warning`. The shell schema did not change (no typed scene or menu field changed).

## What Windows must do

- Ship `plumcraft-text-engine.exe` and `plumcraft-array-engine.exe` beside `Plummer.exe` (and the audio and craft engines' programs to offer them as processes): the core now shows a hosting whose program is missing disabled, "This app doesn't include the Text engine program (plumcraft-text-engine)".
- The C# runner: `$includes` (the got value is text holding the text given; a number is not its digits) and, in `$deep`, each field's want matched as at a pointer; `--has copied_manifest` with the folder `conformance::copied_manifest_folder` makes (probe.toml, copied.toml, the stand-in `uv.cmd`) made by its script, so the gated steps run.
- Draw an item's `warning` (a warning glyph as the `MenuFlyoutItem`'s icon, the text as its tooltip), and `engine.warning` in its engine cell.

## Defects introduced and caught

- The first scripted-copy run set the save panel's folder and name after the panel showed; the out-of-process panel kept its default and the press saved into `~/Documents` (`Untitled.json`, 640 bytes; reported, deleted by the controller). Since then nothing is pressed.
- `PlummerDocumentController.maximumRecentDocumentCount` as a main-actor getter stopped the scripted copy (SIGTRAP) when AppKit read it from its own queue; nonisolated now, with an alive check after a recent document is noted.
- The person check's `menu:` step asked the app's own menus' delegate to fill them, which switched the menu bar to another window (sheets then opened on the wrong window); only Open Recent's delegate is asked now.

## Found and fixed on the way

- B110: a node working on an engine killed mid-node kept its progress on its face after the run ended (the engine half's F35 step).

## Written outside scratch

- `~/Documents/Untitled.json` by the first person-check run (above). Nothing else: every later run's home listing and the person's Library for both ids were unchanged.
- Instruments' raw traces of two B76 probe recordings in the per-user temporary folder (386 MB, 257 MB), removed by the probe and the sweep (my own recordings' files).
- The scripted id's Library locations (its defaults, its recent list), made and removed by the scripts.

## Unverified

- B76 on a device; the iPad's warning marks drawn with a warned engine; Windows, the C# runner.

## The iPad device run, on the simulator

The controller's device run (the main checkout's build): 5 tests, 2 passed, 3 failed, 11 failures, 1,185 s. Here, on an iPad Pro 11-inch (M5) simulator booted by its UDID and shut down after, portrait, headless (the software keyboard shows), this branch's build:

| Test | Device | Simulator, status by file | Simulator, status by element |
|---|---|---|---|
| testCompactWindowsByATap | passed, 37 s | passed, 39 s | passed, 39 s |
| testEveryViewerByATap | failed (B112) | passed, 28 s | passed, 30 s |
| testInlineEdits | passed, 863 s (B111) | passed, 21 s | passed, 22 s |
| testTouchGestures | failed (B113) | passed, 39 s | passed, 40 s |
| testWindowsAndNames | failed (B114) | passed, 20 s | passed, 20 s |

None reproduces, so nothing was changed in the app or the tests, and every one needs a device re-run. What on a device differs: the build (the device ran the main checkout's); real touches at 120 Hz and the timing of a double tap right after launch (B112, B114); a window's width class and the toolbar's items (B113's Select button); the keyboard and its frame notifications (B111). B111's log shows the never-idle starting with the number field's keyboard, before Done; the only endless animation in the canvas is a running face's pulse, and the keyboard lift animates the viewport on each keyboard frame change: the two candidates. No test was found stale against today's pop-out, grid-window and document-window changes: all five pass against them here.

## The gate

All on the Mac, under the scratch environment (`claude-debug/native-n4/s6-env.sh`), on the final code.

| Step | Result |
|---|---|
| `cargo fmt --all --check` | Clean (after `cargo fmt --all`; the first run found my unformatted lines) |
| `cargo clippy --all-targets --all-features -D warnings` | Clean |
| `cargo nextest run --all-features --no-fail-fast` | 1,604 of 1,605; the one failure is the known `plumcraft-craft::params_coverage hidden_params_match_the_baseline` |
| Hash and core-toolkit gates | Both pass |
| Python (`uv run pytest`, ruff) | 230 of 230; ruff clean |
| Private strings | Clean in 1,273 tracked files; the new untracked files checked against the same patterns, clean |
| Shell schema | Regenerated; unchanged |
| xcframework | macOS arm64, iOS and the iOS simulator, rebuilt |
| `swift test` | 97 tests, 0 failed |
| `PlumCraftMac/conformance.sh gate`, three times | Each: 57 of 57 cases (772 steps), then the person check: 47 PASS, 0 FAIL, 1 SEE (Open Recent's items outside tracking); Library and home unchanged |
| Mac Release, iOS Release (device SDK, not signed, not installed) | Both build |
| iPad simulator conformance (`PlumCraftPad/conformance.sh all`, a simulator booted by its UDID and shut down) | 56 of 56, 1 skipped (Install Engine…'s pick list: the iPad lacks install) |
| The five iPad UI tests on that simulator | 5 of 5, twice (status by file, by element) |
| egui's tests | In the full run (`plumcraft-ui`), all pass |

## Decisions for the maintainer

1. The person check opens Open Recent's list as its item does but can't read the menu's items (AppKit fills them only while tracking): accept that SEE, or allow a tracked menu (it would take the pointer).
2. B111 to B114 need a device re-run: should the app report what animates (a running pulse, the momentum link, a viewport animation) in its status for that run?
3. The iPad's engine half: attach a running engine over HTTP (the probe's kind, running for real), or compile the testing kinds into the iPad app.
4. The next piece of the engine half: a test engine with nodes of its own for the port and graph features (F13 to F21, F24, F25), or keep to rows the text engine can show.
