# B134: a way out of Add Node on the iPhone

*2026-11-23, branch `fix/b134-phone-add-node` from `origin/feat/native-n0` (2c3de06), uncommitted. No device.*

## Answer first

On the iPhone in landscape, the Add Node palette was a dead end: UIKit showed its page sheet full screen, with no grabber, no swipe, nothing outside it to tap and no button. In portrait it opened at half height, and only a swipe down or a tap above it closed it; neither is obvious.

The palette sheet now has a Cancel button beside its search field, and in compact height it stays a sheet on the bottom edge, so a swipe down works there too. Each way out closes the palette in the core (`canvas.palette {open: false}`), and the UI test reads it closed in the core and on screen after each.

## Reproduced

On the iPhone 17 Pro simulator at one step larger text (the maintainer's size), with the UI test before the fix:

| | Portrait | Landscape |
|---|---|---|
| How it is shown | page sheet at half height, grabber, no button | full screen: no grabber, no button |
| A swipe down | closes it | nothing (no sheet to drag) |
| A tap above it | closes it | nothing (nothing outside it) |
| A button | none | none |

Pictures before: `claude-debug/n4-decided-2/shots-p1/b134-ui-palette-portrait.png` and `shots-p2/b134-ui-palette-landscape.png` (scratch worktree).

## The fix

All in `PlumCraftCanvas/Palette+UIKit.swift`, the palette's view:

- A **Cancel** button (`palette-cancel`) beside the search field, shown while the palette is a sheet (an iPad's popover closes by a tap outside it). It calls `PaletteModel.dismiss`, which runs the core's `canvas.palette {open: false}`.
- In compact height the sheet stays **attached to the bottom edge** (`prefersEdgeAttachedInCompactHeight`), so it keeps its grabber and a swipe down. It opens at half height where the phone has the height for it.
- In landscape the sheet takes the whole height (UIKit gives a sheet one height in compact height), so there is no graph above it to tap. Cancel and a swipe are the ways out there; the test says so and doesn't tap.

## The other presentations on the phone

I checked the code and pictured them in the phone run below. Every other sheet already has a Done button: the pop-outs (`popout-done`), the parameters (`parameters-done`), Settings, Messages, the Cache Report and Parameters windows, Compare and About. Alerts have their buttons, and menus close by a tap outside. None has the same dead end.

The phone run's pictures are in `claude-debug/n4-decided-2/shots-phone/` (scratch worktree):

| Presentation | Picture | Verdict |
|---|---|---|
| Add Node, portrait and landscape | `b134-ui-palette-portrait.png`, `b134-ui-palette-landscape.png`, `b134-ui-after-swipe-*.png` | fixed |
| A value's sheet (table, YAML, picture, audio, arrays, SVG) | `n4s5-ui-*.png` | Done in its bar |
| Parameters (portrait) | `n4s3-ui-parameters.png`, `n4s6-ui-params.png` | Done; its landscape width is B135 |
| Cache Report, Compare | `n4s6-ui-cache-report.png`, `n4s6-ui-compare.png` | Done |
| Engine › Attach… (a form) | `n4s6-ui-attach.png` | Cancel |
| The More menu, a context menu, a choice's menu | `n4s2-ui-more-menu.png`, `n4s1-ui-context-menu.png`, `n4s2-ui-choice-menu.png` | a tap outside |
| The row editor's keyboard | `n4s2-ui-text-keyboard.png`, `n4s2-ui-number-keyboard.png` | Done in its bar |

A different fault came up and is filed as **B138**, not fixed. On the phone, `testWindowsAndNames` doesn't get into the group: a dropped wire's palette opens instead (`n4s3-ui-inside-group.png`).

## The iPhone, tested from now on

`native/apple/PlumCraftPad/conformance.sh` has a named configuration, `PHONE=1`. It uses an "iPhone 17 Pro" simulator (the maintainer's iPhone's class), boots it if needed and shuts it down after, and runs at one step larger text unless `TEXT_SIZE` says otherwise. For example: `PHONE=1 native/apple/PlumCraftPad/conformance.sh gestures`.

Every UI test on the phone took **295 s** with the build:

| Test | Result |
|---|---|
| `testAWarnedEngineIsMarkedInTheMenu` | passed, 23.5 s |
| `testCompactWindowsByATap` | passed, 26.0 s |
| `testEveryViewerByATap` | passed, 45.0 s |
| `testInlineEdits` | passed, 21.1 s |
| `testLeavingAddNodeInCompactWidth` | passed, 30.9 s (new) |
| `testNoKeyboardWithNothingEdited` | passed, 25.3 s |
| `testTheSavesQuestionAndARecipe` | skipped (no test engine) |
| `testTouchGestures` | passed, 39.7 s |
| `testWindowsAndNames` | failed: B138 |

## Gate

| Check | Expected | Took | Result |
|---|---|---|---|
| iPad standard: device build, whole suite, UI tests | 8 min | 499 s | build passed; 59 of 59; UI 8 passed, 1 skipped, 0 failed (`testLeavingAddNodeInCompactWidth` included, in a compact window) |
| Every UI test on the iPhone | 5 min | 295 s | 7 passed, 1 skipped, 1 failed (B138) |

No Rust, Mac or kit code changed, so there was no Rust or Mac gate. The palette file is iOS-only.

## Brought up to the dev branch

The branch was brought up to `ede5705` (#175, #176) with no commit: a fast-forward, then the work put back with a three-way apply. `docs/BUGS.md` and `docs/STATUS.md` conflicted, and both sides were kept. The new bug is **B138**: B136 was taken in #176, and B137 is B133's. B135's walk found that `testWindowsAndNames` passes on the phone in landscape, so B138 belongs to the portrait layout. The closing round after the merge (`b134gate.sh`, 11 minutes):

| Check | Took | Result |
|---|---|---|
| iPad standard: device build, whole suite, UI tests | 339 s | 59 of 59; UI 8 passed, 1 skipped, 0 failed; home listing unchanged |
| Every UI test on the iPhone | 290 s | the same as before: 7 passed, 1 skipped, `testWindowsAndNames` failed (B138) |

The xcframework step found the shared cargo target current, so these runs linked the core as last built there, which includes B133's additions to it. B134 changes no Rust.

## Unverified

Nothing ran on a device.
