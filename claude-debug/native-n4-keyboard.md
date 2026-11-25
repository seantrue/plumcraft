# The stray keyboard (B118) and the next device run

Branch `native/n4-keyboard` from `feat/native-n0` (at #160), 2026-11-22, uncommitted.

## Answer first

B118 is the system's, not the app's. On the iPad simulator with the software keyboard on, no keyboard is up after a launch; one comes up whenever a menu is open and goes when it closes. The stock Files app's More menu does the same. The earlier pictures were taken with the More menu open. B118 explains none of B111 to B114.

## Decisions recorded (note 24)

| # | Question | Answer |
|---|---|---|
| 1 | egui and the audio engine | Leave it as a process |
| 2 | The test engine on the iPad | Leave it Mac-only |
| 3 | F19, save formats in the Rust SDK | Open ("which rust sdk?"); nothing built |
| 4 | B118 | Look now |

The answer line, verbatim: "1 - leave as process; 2 - leave as mac only; 3 - which rust sdk?; 4 - look now".

## B118

The iPad app now writes `keyboard` (shown, frame) and `firstResponder` (type, identifier) to its status whenever the keyboard shows, hides or moves (`KeyboardWatch` in `AppMain.swift`, debug and release builds).

| Probe (iPad Pro 11-inch (M5) simulator, booted here) | Keyboard |
|---|---|
| After launch, Inline Editing and Viewers samples, alone and after the iPad conformance (ten launches) | none |
| The bar's More menu open | up, first responder the graph controller, no text or search field |
| The More menu closed | none |
| The title menu open, then closed | up, then none |
| The same with the controller not made first responder, with no key commands, with editing interactions on | the same |
| The stock Files app's More menu open | up |

New UI test `testNoKeyboardWithNothingEdited`: no keyboard after the launch, after a menu opened and closed, and after a text field's Done; one while the field is edited. The warned-engine test no longer taps Hide keyboard, and skips itself on a device (the runner's files are in its own sandbox there). Proposed: close B118 (the maintainer's decision).

## B111 to B114 in that light

- B111: every 60 s idle wait in the device log is in `testInlineEdits`. The first starts a fraction of a second after `7` was typed into the number field (the typing's own idle wait ended at 12.02 s; the wait before the tap on Done never did). The field was still open and no menu was open. The next test's launch has no idle wait. The log has no keyboard or first-responder data, so B118 does not explain it. The candidates stay the keyboard lift and the number keyboard's frame changes feeding each other, or a face left running.
- B112 to B114: the element lists at the failures show no keyboard. B113's missing Select button was the bar's overflow (the More button was there), a width matter.
- Time-outs in the UI tests now print `animating`, `keyboard` and `firstResponder`. `testInlineEdits` prints a `B111:` line either way, and `device.sh` and `conformance.sh` keep it in their logs.

## The next device run

One command, from a checkout of this branch once it is merged, with the iPad unlocked (Auto-Lock Never, UI Automation on, VPN off):

```sh
DEVICE=<the iPad's UDID> TEAM=<team id> OUT=claude-debug/native-n4/s10-device native/apple/PlumCraftPad/device.sh ui
```

It builds and installs the branch's Debug app and test runner (`build-device/test`). It runs seven tests: the five earlier ones, `testNoKeyboardWithNothingEdited`, and `testAWarnedEngineIsMarkedInTheMenu`, which skips itself on a device. It prints each result, each failure with `animating`, `keyboard` and `firstResponder`, and the `B111:` line (`[]` when idle). The details are in note 24.

## BUGS

B90 and B91 moved to Fixed (merged in PR #158; B91 verified by a connected run on the Windows test VM). B111 and B118 updated.

## Gate

The screen was unlocked (`IOConsoleLocked` false) at the start and after the Mac gate, so every run had accessibility. No Rust or Python changed, so the xcframework and the Rust and Python gates were not re-run.

| Run | Result |
|---|---|
| The iPad's device build (`build-for-testing`, generic iOS, no signing, nothing installed) | built |
| The Mac gate once: the suite on the app's control port | 63 of 63 cases, 909 steps |
| The Mac person check | 50 passed, 0 failed (Save never pressed) |
| The iPad conformance on the simulator | 59 of 59 cases, 4 skipped (as before) |
| The iPad UI tests on the simulator | 7 of 7 passed; `B111: animating 8 s after the number's Done: []` |
| The new keyboard tests alone, before the gate | 2 of 2 passed |
| The home and Library listings, before and after | unchanged |
| The private-strings check | no match in 1,289 tracked files |

Not run: the device (no physical devices), the Linux and Windows gates (nothing they build changed).
