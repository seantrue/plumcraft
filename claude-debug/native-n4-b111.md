# B111 read from the device, B129 found and fixed, B130 at the maintainer's text size

*2026-11-22, branch `native/n4-b111` from `origin/feat/native-n0` (4366fe9), uncommitted. No device was used.*

## Answers first

- **B111 (time-boxed to 30 minutes, from 21:53):** the text cursor's blink is not the cause, and nothing of ours was found to fix. The device's own keyboard animations, its arrival and its going, stay attached long after their 0.38 s; the simulator has none of them. B111 stays open; the probe now says, for each animation, whether it has ended and is kept, so the next device run can tell UIKit keeping finished animations from animations that never run.
- **B129 found and fixed at source:** a sheet replaced by another gave the keyboard focus back to the responder of before, which ended the row field's editing, and the field then wrote the text it was opened with over a value set meanwhile. A focus loss now commits only what was typed (iPad and Mac). The whole iPad suite with the test engine: 65 of 65 (3 of 3 failures before).
- **B130 at one step larger text:** the fold reproduces at exactly the maintainer's size at 375 by 655 with the fix off, and not with it.
- **B60 closed** as decided (10A): tonight's device run passed `testWindowsAndNames` with the window set.
- **At one step larger:** the eight UI tests pass on the whole screen. At 375 by 655, six of eight passed in the closing run. The two failures, both in tests a device run skips:
  - `testAWarnedEngineIsMarkedInTheMenu`: the long Engine menu over the simulator's keyboard didn't scroll to the warned engine. It passed on the whole screen.
  - `testTheSavesQuestionAndARecipe`: a lost element in its Recipe step. It passed at 375 in an earlier run.

  Both are not fixed.

## Tonight's device run

The merged build `4366fe9`, the window left at 375 by 655 and made whole by each launch: 8 tests, 6 passed, 2 skipped (the warned-engine test and the save-question test, which a device run can't give), 0 failed, in 181 s, with XCTest's idle wait skipped and no idle-wait timeout. `testInlineEdits` took 17.6 s (859 s the night before). Recorded in note 24 with the maintainer's decision.

## B111

| Moment | The device (iPadOS 27) | The simulator (iPadOS 26.5) |
|---|---|---|
| With the number typed | the cursor's blink (`_uitcvba`, 1 s, repeating forever); the keyboard's arrival (0.3833 s: item container, backdrop, input assistant, Liquid Glass `effect.*`); the glass's `match-*` | the cursor's blink; the glass's `match-*` |
| 8 s after Done | the keyboard's going still attached (`position-2`, `cornerRadii`, the accessory bar's `UIToolbar` `position`, a 0.47 s `opacity`, `match-*` on `GlowContainerView`) | only the glass's `match-*` |

1. **The cursor's blink:** on both, gone after Done on both, and XCTest reaches idle on the simulator with it present. Not the cause. Ours is a plain `UITextField` subclass, resigned and removed from the view tree on Done; no hidden first responder is kept.
2. **The keyboard's animations 8 s after Done:** the device alone has them. Whether they are over but kept, or never ran, the probe could not say; it now records `age`, `ended`, `kept` (not removed on completion) and `fill` for each animation, and each window's class, hidden state and alpha. Checked on the simulator that the new fields read (the glass's `match-*`: kept, fill both, not ended, as expected of a matching animation).
3. **Ours to fix?** Not shown. The animations belong to UIKit's keyboard; the one view of ours among them is the field's input accessory bar, which the keyboard animates with it.

**The next four-minute device run should check:** the same eight tests (`device.sh ui`), reading the `B111:` lines: if the leftover keyboard animations are `ended` and `kept`, UIKit keeps finished animations (not ours to fix; skipping the idle wait stays the answer); if not `ended` after seconds, they never ran, and the window list shows whether their window is hidden. If a minute can be spared, `testInlineEdits` alone with `PLUMCRAFT_IDLE_WAIT=on`, to see whether XCTest's wait still stalls on this build.

## B129

The field logged why its editing ended, with the call stack (a scratch build only; the logging is not kept). It was still in the key window, the scene active, nothing finishing it, and UIKit's own stack took the focus: `GraphViewController.replace(with:)` dismissing a presented sheet to present another (`presentReplacing`, a pop-out's sheet; the test engine's cases leave pop-outs open), and the dismissal restoring the input views of before the presentation (`_restoreInputViewsForPresentation` → `UIKeyboardSceneDelegate _restoreInputViewsWithId:animated:` → `becomeFirstResponder`). The field's focus loss then committed "3" over the 7 the script had just set. The earlier guess (a scene change) was wrong.

The fix (`FieldEditorHost.lostFocus`, and the same on the Mac in `RowEditor+AppKit.swift`): a focus loss commits only what the person typed; a field left as it opened sets nothing and stays open (the core still has it open; a tap on the canvas closes it). A first version closed the editor instead, which broke the case's later steps (it expects the editor open), so it stays open. Not covered: the field's text doesn't follow a value changed from outside while it is open; the person sees the old text until they type.

## B130 at one step larger

`TEXT_SIZE` is now a named setting of `PlumCraftPad/conformance.sh` (and the scratch scripts): `default` is `large`, `one-step-larger` is `extra-large` (Settings › Display & Brightness › Text Size one position past the default: the maintainer's iPad); the simulator's size is put back after.

- Before the fix (switched off for one run), 375 by 655 landscape: `testNoKeyboardWithNothingEdited` failed at "the bar's More menu". After: passed in every run.
- `testInlineEdits` failed at that size: the face's more-button was off a 375-point window; the test now pans it into reach first.
- The save-question test: the export picker in a narrow window is a whole-window sheet with no Cancel; the test closes it by pulling it down from its top (never Save).

## Running without XCTest's idle wait: what it cost

At one step larger and 375 points, taps sent while a sheet, menu or keyboard was still moving were lost, test by test. The tests now settle 300 ms before each action in place of XCTest's wait (`PLUMCRAFT_SETTLE_MS`), retry a More tap whose menu didn't open, retry a lost wire drag or double tap, wait for an alert to go before touching the canvas, and pick the hittable one of several elements with the same words.

## Gate

The closing gate (`claude-debug/n4-decided-2/final9.sh`, expected 35 minutes, took about 45):

| Check | Expected | Took | Result |
|---|---|---|---|
| swift test | 2 min | 36 s | passed |
| Mac gate, twice (second under 10 busy loops) | 6 min | 583 s | 66 of 66 cases each; person check 56 passed, 0 failed, 8 only seen (the screen was locked: no accessibility) |
| iPad standard: device build, whole suite, UI tests | 5 min | 287 s | build passed; 59 of 59 cases; UI 7 passed, 0 failed, 1 skipped |
| iPad whole suite with the test engine (B129) | 1 min | 51 s | 65 of 65 |
| The eight, default text, whole screen | 4 min | 252 s | 8 of 8 (21.0, 26.5, 47.3, 19.3, 25.3, 27.5, 37.7, 32.6 s) |
| The eight, one step larger, whole screen | 4 min | 253 s | 8 of 8 |
| The eight, one step larger, 375 by 655 landscape | 8 min | 499 s | 6 of 8 (above) |

The Rust gate was not run: nothing in Rust changed on this branch. The private-strings check found no match. The home listings before and after both Mac gates were unchanged. The person's engines folder was unchanged, and the scripted copy's Library was cleaned.

## Durations

| Run | Expected | Took |
|---|---|---|
| The probe read on the simulator (`testInlineEdits`, element mode) | 3 min | about 3 min |
| Each whole iPad suite with the test engine, while finding B129 | 2 min | 51 to 59 s |
| Each 375 run at one step larger (Settings switched and back, eight tests) | 8 min | 483 to 589 s |
| Each whole-screen run of the eight | 4 min | 250 to 310 s |
| One stalled run of the eight (a pop-out wait of 120 s per node; stopped by its PID) | 4 min | about 10 min before it was stopped |

## Unverified

- Nothing on a device: B111's new fields, the 300 ms settle on a device, B130's iPadOS 27 path, B129's fix there.
- B129's fix on the Mac is not exercised by a test that changes a value under an open field.
