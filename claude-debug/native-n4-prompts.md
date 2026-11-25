# The prompts' own words, Recipe and the save's question on the Apple shells; the idle wait

*2026-11-22, branch `native/n4-prompts` from `origin/feat/native-n0` (a4bfb5e), uncommitted. No device was used.*

## What was asked, and what came of it

| Item | Outcome |
|---|---|
| Prompts use the core's `labels` | Done on the Mac and iPad (`AskWords.buttonTitle(_:prompt:)`, falling back to the platform's words); `saveShape` pictured on both |
| Mac person check: Traced and Recipe | Done: Traced popped out by a click on its pin, Recipe pressed through accessibility, "traced" read from the popover; none on the Result node's value |
| Mac person check: Strip and the question | Done: Save Result… from the footer, PNG by the scripted file answer, the prompt's three labelled buttons read through accessibility and pictured; "Save as NumPy (.npy)" writes a `.npy`, the second pass "Save as PNG anyway" a PNG |
| Swift runner, the two new ports cases | They run on the launched Mac app as they are (it already claims `engine:ports`, `ports_installed`, `saves`): both pass |
| iPad and the test engine | A script change only: `PAD_PORTS=1` starts the test engine on the Mac serving HTTP on loopback and attaches it by URL; the ports cases and a new UI test run on the simulator |
| B111, ten minutes | Not solved; what is known is in B111 |
| The idle wait | Skipped: the UI tests wait on the app's state; the debug build reports running layer animations when asked |
| The 375-wide failure | Reproduced on the simulator at a large text size and fixed (B130) |

## Prompts

`ShellWords.swift` gains `AskWords.buttonTitle(_:prompt:)`: the prompt's `labels[answer]` when given and not blank, else the platform's words (`buttonTitle(_:kind:)`, as before). The Mac's `NSAlert` and the iPad's `UIAlertController` both use it; a unit test covers the labels and the fallback. `docs/shell-interface.md` 11.2's `prompt` row names `labels`.

The core's prompt text began with the engine's clause in lower case ("this 4 by 1000 array is…", seen in the Mac picture): B127, fixed in `Prompt::text` with a test.

## The Mac person check

New steps, after Save Result, with the test engine installed as before:

- **F56.** A graph of Traced (seed "corgi") and Result is opened as the Finder opens a file and run from the Run menu. A click on Traced's output pin pops its value out; the app says `hasRecipe: true`; the Recipe button is pressed through accessibility (`person-check-ax press`); the popover's text is read through accessibility and from the app's report ("traced"). The Result node's value, popped out with View › Pop Out's command (it has no output pin), says `hasRecipe: false`, and a press of `viewer-recipe` there finds no button.
- **The save's question.** Strip is opened and run; a click on its footer's Save Result… line (the core's gesture) raises the save ask, answered by the script with a scratch `.png` path (the panel is never pressed); the engine asks back; the app's own alert sheet shows "Save as PNG anyway", "Save as NumPy (.npy)", "Cancel", read from the sheet and through accessibility, and pictured. The script presses "Save as NumPy (.npy)" in that sheet and reads back a file starting `\x93NUMPY`; a second pass presses "Save as PNG anyway" and reads a PNG.

## The iPad and the test engine over HTTP

No app or core work was needed. The test engine's program already serves HTTP (`--listen 127.0.0.1:0 --port-file f`, its token from `PLUMCRAFT_ENGINE_TOKEN`); the simulator shares the Mac's loopback; the app's engines folder (`PLUMCRAFT_ENGINE_FOLDER`) takes a manifest of mode `http` with its URL and token file, and the iPad core connects to it without starting a program (contract 4.2). `PlumCraftPad/conformance.sh` does this with `PAD_PORTS=1` (decision 2 kept the test engine Mac-only as a program; this keeps it a Mac process). With it the ports cases claim `engine:ports`, `ports_installed` and `saves`, and the gestures run gets `PLUMCRAFT_PORTS_ENGINES` for the new UI test.

The new UI test, `testTheSavesQuestionAndARecipe` (skipped without the engine), in the compact layout: Strip run, its footer's Save Result… tapped, the alert's name set to `strip.png`, Save; the question's alert with the three labelled buttons (pictured); "Save as NumPy (.npy)" tapped; the export picker offers `strip.npy` (cancelled). Traced's value popped out by a double tap on its pin, Recipe tapped, "traced" read on screen, the sheets pulled away; Strip's value has no recipe.

Two iPad defects came out of it, both fixed in the shells: **B126** (after the engine's question the iPad said "Couldn't save" and never offered the file the answer wrote) and **B128** (the Recipe sheet drew "No recipe" over a recipe the app had; the Mac's popover now is read through accessibility too).

**B129, open:** with the test engine attached, the whole iPad suite fails "a value typed into a row is checked…" (the value set goes back to the field's), three runs of three; it passes in the edit half alone with the engine and in the whole suite without it.

## B111 and the idle wait

The maintainer: "Efficiency in testing is important too. I suggest making B111 the next order of business, and if a time boxed 10 minutes doesnt solve it, make a note of the issue and go to skipping the idle wait." and "mindful in the future of this kind of clock wasting. Time is precious." Both are recorded in note 24.

**B111, ten minutes from 20:33, the logs and the code only: not solved.** In both device runs the first wait that never ends follows text typed into a field of the canvas (a `KeyedTextField`, the software keyboard up): in `testInlineEdits` after `7` was typed into the number field, every pre- and post-event wait then ran its 60 s until the app was terminated (14 waits, 859 s; 18 at 375 wide); in `testNoKeyboardWithNothingEdited` the same after `no keyboard` was typed, through Done (3 waits). The app's own `animating` was empty at the one dump taken; no test wait timed out, so `keyboard` and `firstResponder` were never dumped at a stall. Unchecked: the field's caret and the keyboard's own layers, and display links outside the app's `Activity`.

**The idle wait skipped.** XCTest has no public switch. Its own `XCUIApplicationProcess` asks `shouldSkipPreEventQuiescence` and `shouldSkipPostEventQuiescence` before and after each action (and logs "Skipping pre-event quiescence check" when told yes); the UI tests make both answer yes (`IdleWait` in `GestureTests.swift`, set up before every test, printing `IDLE: XCTest's idle waits skipped (2 of 2)`; `PLUMCRAFT_IDLE_WAIT=on` restores the waits). What it does not cover: the wait for SpringBoard to idle before each action (the interruption check), and launch's own wait for the app. The tests already wait on the app's own state (the status, an element's existence), each wait with its own limit (20 s by default, at most 150 s for a launch); one wait had leaned on XCTest's: the keyboard check now waits up to 5 s for a keyboard to go after Done before it watches for one coming.

**Evidence for the next device run, at no cost.** The iPad's debug build has a second element beside the status element, `plumcraft-layers`, whose value is worked out only when accessibility reads it: every layer animation in the app's windows (layer, owner, key, repeat count, duration) and the display links the app knows of. The UI tests read it when a wait times out and at the point where B111 began (`B111: layers with the number typed: …`), on a device, where the status is read from the element.

## The 375-wide failure (B130)

On the device at 375 by 655, `testNoKeyboardWithNothingEdited` found no `tool.more`: the bar had Documents, the title and the system's own overflow button. On the simulator at the default text size it does not happen (w1, where the test failed instead on the keyboard race above, since fixed); at extra-extra-large text it happens every time (w2, w3), so the device's text size is the likely difference. UIKit folded Run and More into its overflow. Neither the iPadOS 27 visibility priority (the simulator is 26.5, so not exercised) nor the navigator style's centred title (w4) kept them. The fix: in a narrow compact window at a text size above the default, the bar keeps More alone and Run goes first into its menu; after it the test passes (w5, 20.7 s), and again run straight after `testInlineEdits` with the final test file (w6, 23.8 s). At that text size two other things fail that are not the device's failure and were left: `testCompactWindowsByATap` doesn't find Settings in the longer More menu, and `testInlineEdits` doesn't open the node's menu from the more-button.

## Floor

| | Met | Partly | Not met |
|---|---|---|---|
| Mac, before | 51 | 3 | 31 |
| Mac, now | 52 | 2 | 31 |
| iPad, before | 38 | 1 | 46 |
| iPad, now | 47 | 2 | 36 |

F56 is met on both (Mac launched; iPad launched over HTTP). On the iPad, over HTTP: F13, F14, F15, F17, F18, F19 (Save Result), F20, F21 met and F16 partly, from the ports cases on the simulator's app.

## Gate

The closing gate (`final8.sh`) and the runs after its UI-test fixes:

| Check | Result |
|---|---|
| fmt, clippy, schema, hash gates, core-toolkit gate | clean |
| nextest | 1,645 of 1,647: the known `hidden_params_match_the_baseline`, and `kittest_viewer_tools scrolling_in_past_the_picture_asks_for_detail_once` once (passed alone at once; B131) |
| pytest, ruff | 249 passed; clean |
| private strings | no match |
| swift test (the kit) | passed |
| Mac gate, run 1 | 66 of 66 cases; person check 58 passed, 0 failed |
| Mac gate, run 2, under 10 busy loops | 66 of 66 cases; person check 58 passed, 0 failed |
| iOS device build-for-testing | passed |
| iPad conformance, whole suite (standard) | 59 of 59 passed, 7 skipped (the test engine's and Install's) |
| iPad conformance, edit half with the test engine | 29 of 29 |
| iPad UI tests, the eight, with the test engine, no idle wait (q7) | 8 of 8 passed |
| iPad UI tests at 375 by 655 landscape, extra-extra-large text | see "The 375-wide failure" |

In the gate's own UI run, before the last test fixes, `testCompactWindowsByATap` and `testTouchGestures` failed: with XCTest's idle wait gone, a tap on More while a sheet was still going, and a look for a bar button while the bar was still settling, were lost. The tests now wait for either (`toolbar`, `item`), retry a More tap whose menu didn't open (`fromMore`), wait for an alert to go before touching the canvas, and try a lost double tap again; `testEveryViewerByATap`'s pop-out wait is 30 s, not 120 (one run stalled there for ten minutes and was stopped by its PID). The eight then passed (q7).

## Durations

| Run | Expected | Took |
|---|---|---|
| The eight iPad UI tests, simulator, no idle wait (q7) | 4 min | 227 s with the build and boot; the tests 210 s (13.5, 31.9, 42.0, 15.7, 23.5, 24.0, 32.1, 27.6 s) |
| The same seven on the device before (run 1, full screen) | | 1,219 s |
| Mac gate, each run | 3 min | about 2 min |
| Rust gate | 10 min | a few minutes (built before) |
| iPad conformance, whole suite | 5 min | about 4 min |
| The 375 repro (Settings switched to Windowed Apps and back, two tests) | 6 min | about 5 min |

With the idle wait gone, a device run of the eight should take about four minutes, unless B111's stall now shows up somewhere the tests wait on the app (it would then fail at that wait's own limit, with `plumcraft-layers` in the message). Not run on a device.

## Accessibility and home

Every launch run here had accessibility: the screen was unlocked (`IOConsoleLocked` false) at the start of each Mac gate and round. The home listings before and after every Mac gate were the same ("nothing came or went"), the scripted copy's Library was cleaned, and the person's engines folder was unchanged; the iPad rounds' home diffs were empty. The simulator used was the one booted here by UDID and shut down after each run; its text size was put back to "large" after the extra-extra-large runs and its multitasking setting back to Full Screen Apps.

## Unverified

- Nothing on a device: B111's new evidence, the skipped idle wait's four-minute device run, and B130's iPadOS 27 path are untried there.
- B129 is not understood.
- The Windows shell is untouched (its `Word` still derives the prompt's words from the answers' names; `person-check.ps1` has no Traced or Strip step).

## Decisions for the maintainer

1. B129: look into it now, or leave the test engine's iPad run to the edit half until it is?
2. B130: is Run inside More acceptable in a narrow window at a large text size, or should the title give way instead (a custom title view, losing the document menu)?
