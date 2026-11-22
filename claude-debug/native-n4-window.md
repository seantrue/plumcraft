# The iPad in a free-size window

Branch `native/n4-window` from `feat/native-n0` (at PR #163), 2026-11-22, uncommitted.

## Answer first

Both device runs failed because the iPad is on Windowed Apps. The maintainer: "It's on Windowed apps". With that setting the app opens in a free-size window. iPadOS keeps whatever size the window last had, and here that was 375 by 655.5.

The tests never asked for a size. `testTouchGestures` turns the device, and a turn changes a free-size window's size, so later tests and later runs inherited whatever the turns left.

At 375 by 655 on the simulator with Windowed Apps, all seven tests failed as on the device. The causes:

- two app faults, B119 and B120, both fixed;
- the tests' whole-screen assumptions, B121, fixed.

The window does not explain B111.

## What each test had

| Test | Run A (before #163) | Run B (#163) |
|---|---|---|
| `testAWarnedEngineIsMarkedInTheMenu` | failed | skipped (device) |
| `testCompactWindowsByATap` | failed (no size printed) | failed, 375 (pictures) |
| `testEveryViewerByATap` | failed, 375 by 655.5 | failed, 375 by 655.5 |
| `testInlineEdits` | passed, 859 s (no size printed) | passed, 21 s, 375 (pictures) |
| `testNoKeyboardWithNothingEdited` | (not in that build) | failed, 375 (pictures) |
| `testTouchGestures` | failed, 375 by 655.5 | passed; whole screen after its turns |
| `testWindowsAndNames` | failed, 375 by 655.5 | passed, whole screen |

Yesterday's run started at 1,067 by 655.5 and was 375 by 655.5 after `testTouchGestures`' turns.

## App faults, fixed

- **B119, the narrow bar.** In a 375-point window the window controls sit beside Documents. Documents, Add Node, the title, Run and More don't all fit, so the system folded Run and More into its own overflow menu. The fix: a narrow window (compact, under 440 points) keeps Documents, the title, Run and More, and moves Add Node into the More menu.
- **B120, the keyboard lift.** The keyboard's frame is in screen coordinates, but the row editor converted it as if it were in window coordinates. In a window that isn't at the screen's origin, the field being edited stayed under the keyboard. It is now converted from the screen's coordinate space.

## Tests, fixed (B121)

- **Window size at launch.** Each launch asks for its window with `--window-size`:
  - the whole screen by default, reached through the system's window controls (Zoom) when the window isn't already the whole screen;
  - `PLUMCRAFT_WINDOW=WxH` asks for a smaller size, reached by the app pinning its scene's size and the test dragging the window's corner.
- **Orientation.** `PLUMCRAFT_ORIENTATION=landscape` turns the device first.
- **Points in reach.** Points are panned into reach before a tap, as a finger would, from empty canvas. The app's status now gives each face's box.
- **Toolbar buttons.** They come from the bar or from its More menu.
- **Taps outside a menu or the palette.** They go to the canvas's top left, below the bar.
- **Rotation.** In a free-size window, a turn checks that the graph is still shown, not the window's shape.
- **The viewer test.** It opens each viewer's node centred, so it works at any size.
- **The warned-engine test.** It asks for the whole screen, upright, whatever `PLUMCRAFT_WINDOW` says. This test runs on the simulator only.
- **Every log says the window.** The status has `scene`: size, size classes, activation state, orientation, whether it is the whole screen, and the size asked for. Every test prints `WINDOW …` once ready, and `testTouchGestures` prints it after each turn. Every time-out carries it too.

## What the simulator's iPadOS offers

- **Windowing mode.** Settings › Multitasking & Gestures offers Full Screen Apps, Windowed Apps and Stage Manager. A UI test can choose one. A SpringBoard defaults key alone gave Stage Manager.
- **No size API on an iPad.** iPadOS has no API to set a window's size; the iOS geometry preferences take orientations only.
- **The scene's size limits.** `sizeRestrictions` pinned at a size makes a corner drag stop exactly there (375 by 655), and the next launch keeps that size. The minimum alone grows a window only in steps, so it isn't relied on.
- **The window controls.** SpringBoard's controls (`window-controls:<bundle id>`, then Zoom) make a window the whole screen.

## B111 to B114, B60

- **B111.** The window doesn't explain it:
  - Run B was in the same 375-point window, with the number keyboard up, and had no idle wait.
  - On the simulator at 375 by 655, `testInlineEdits` passes in portrait, and three times with the device turned landscape (20 to 26 s).
  - Run A's `animating` stayed empty, so the wait was not one of the app's own animations.
  - B111 stays open.
- **B112.** The table's pin was above the window's top. Test fault.
- **B113.** The test's taps and toolbar assumed the whole screen. Test fault.
- **B114.** The group's title was off the window. Test fault.
- **B60.** Its step passed on the device in run B, at the whole screen. Proposed: close once a run with the window set passes it again.

## The next device run

```sh
DEVICE=<the iPad's UDID> TEAM=<team id> OUT=claude-debug/native-n4/s11-device native/apple/PlumCraftPad/device.sh ui
```

Every launch asks for the whole screen, so each `WINDOW` line should say `fullScreen 1`. Then run the same command with `PLUMCRAFT_WINDOW=375x655 PLUMCRAFT_ORIENTATION=landscape` in front, for the maintainer's own window. That run leaves the window at 375 by 655, and the next default run makes it whole again.

## Gate

The screen was locked (`IOConsoleLocked` true) for the Mac gate, so the person check ran without accessibility: its accessibility steps were SEE lines, as designed. No Rust or Python changed, so the xcframework and the Rust and Python gates were not re-run.

| Run | Result |
|---|---|
| The iPad's device build (`build-for-testing`, generic iOS, no signing, nothing installed) | built |
| The Mac gate once: the suite on the app's control port | 63 of 63 cases, 909 steps |
| The Mac person check (locked screen) | 48 passed, 6 seen, 0 failed; Save never pressed |
| The iPad conformance on the simulator | 59 of 59 cases, 4 skipped (as before) |
| The iPad UI tests, Full Screen Apps (the gate's run) | 6 of 7: `testTouchGestures` timed out waiting for the dropped wire's palette once |
| The same seven again, Full Screen Apps | 7 of 7 |
| `testTouchGestures` alone twice, Full Screen Apps | 2 of 2 |
| Windowed Apps, 375 by 655, landscape | 7 of 7 |
| Windowed Apps, the whole screen | 6 of 7 (the turn fault, then fixed: `testTouchGestures` and `testWindowsAndNames` pass after it) |
| `testInlineEdits`, 375 by 655, landscape, three times | 3 of 3, no idle wait |
| The home and Library listings, before and after the Mac gate | unchanged |
| The private-strings check | no match in tracked files |

The simulator's multitasking mode was set back to Full Screen Apps, and the simulator was shut down. Not run: the device (no physical devices), the Linux and Windows gates (nothing they build changed).
