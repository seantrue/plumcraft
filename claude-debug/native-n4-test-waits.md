# The UI tests wait on the app's state (decisions 7A and 8B)

*2026-11-23, branch `native/n4-test-waits` from `origin/feat/native-n0`, brought up to `d29630c` (#180, #181). Uncommitted. No device.*

## Answer first

Before each action, the iPad and iPhone UI tests no longer pause a fixed 300 ms. They wait until the app's own status says nothing is still moving, and the passes are shorter in every configuration:

| Configuration | Before, tests (wall) | After, tests (wall) | Waits | Time spent waiting |
|---|---|---|---|---|
| iPad, whole screen, default text | 300 s (319 s) | 259 s (284 s) | 120 | 8.6 s, was 36 s fixed |
| iPad, whole screen, one step larger | 288 s (307 s) | 261 s (277 s) | 121 | 9.0 s, was 36 s |
| iPad, 375 by 655 window turned, one step larger | 484 s (593 s) | 425 s (1,118 s, see below) | 411 | 32.0 s, was 123 s |
| iPhone 17 Pro, one step larger | 318 s (348 s) | 293 s (323 s) | 129 | 12.2 s, was 39 s |

"Tests" is the sum of the eleven test cases' own times, which is what the waits change. "Wall" adds the build, plus for the 375 window two runs that switch the simulator's Settings to Windowed Apps and back. The 375 after-run's wall time (1,118 s) went to those switches and builds while another agent's build shared the machine; its tests took 425 s. No wait reached its 3 s limit, and the longest was 0.57 s.

On 7A, the two tests at one step larger in the 375 window: their test-side faults are fixed, and both now pass on the whole screen and on the phone. **At 375 both still fail**, for two causes that are now known; see "The two tests" below.

## What changed

**The app** (`PlumCraftPad/App/Settling.swift`, started in `AppMain`) writes `settling` to its status: a sorted list, empty when nothing is moving.

- `presenting`: a view controller is being presented or dismissed, or has a transition under way. This covers sheets, popovers, alerts and pushes.
- `keyboard`: the on-screen keyboard is between its will- and did- notifications, at most the animation's own length.
- `menu`: a menu's container still has animations on its layers.

A display link looks once a frame, and only while something reads the status.

**The tests** (`GestureTests.swift`, `IdleWait`): XCTest's pre-event hook now polls the status file until `settling` is empty, at most `PLUMCRAFT_SETTLE_LIMIT_MS` (default 3,000). On a device the runner reads the status through accessibility, which the hook mustn't query, so there `PLUMCRAFT_SETTLE_MS` (300 ms fixed) stays. Each test logs `SETTLE: n waits, total, longest, at the limit`.

**Why the full comparison took 45 minutes, and what answers the question.** One pass at each of four configurations, each with its own build (about 1 minute), runs eleven tests of about 5 minutes. The 375 window adds two Settings-switching runs, each with a build (about 4 minutes). That makes about 30 minutes of work, and a slow build under another agent's load stretched it. One pass each is all the comparison needs, and that is what is reported. I ran the after pass twice in all: the first one showed my own breakage of the warned-engine test (below), so I reverted that change and ran it again.

## The merge, before my changes

The dev branch as merged (`7fad6dc`, with #177, #178 and #179) built and ran at all four configurations before any change of mine:

- The whole screen, at both text sizes: all eleven passed.
- The 375 window turned, at one step larger: three failed.
  - `testBrowseForAPath` (B133) and `testLeavingAddNodeInCompactWidth` (B134) failed because their taps were measured from the screen's corner, not the window's. In a window smaller than the screen they missed the picker's ✕ and the dimmed graph above the palette. Both are **fixed on this branch** and pass at 375.
  - `testTheSavesQuestionAndARecipe` failed as before the merge.
- The iPhone: `testTheSavesQuestionAndARecipe` and `testWindowsAndNames` (B138) failed.

Nothing the merge itself changed broke a test. The three new tests simply hadn't been run in a 375 window or, for the save test, on the phone.

## The two tests (7A)

- **`testTheSavesQuestionAndARecipe`** failed at 375 and on the phone with "Failed to get matching snapshot: No matches found for Element at index 4 (or 5)". The test reads elements by index while a bar or the export picker is still settling, and one is gone by the time it is read. On the phone the culprit was the test's own diagnostic listing of the picker's buttons.
  - Fixed: the toolbar's Run, that listing, the menu's lists and the Recipe button are now found by the element itself (`allElementsBoundByAccessibilityElement`) or tapped where they were read.
- **`testAWarnedEngineIsMarkedInTheMenu`** is flaky at one step larger, not broken.
  - Before my changes, on the merged dev branch, it passed in all four configurations.
  - On the earlier dev branch it failed at 375 and on the whole screen at one step larger.
  - In my final pass it failed only in the 375 window.
  - The Engine menu, scrolled under the simulator's keyboard (which iPadOS brings up with any open menu, B118), sometimes closes or overshoots.
  - My first attempt (a flick, re-opening the menu, stepping back up) made it fail everywhere, because "Clear Engines" is at the menu's top, not its foot. I reverted it. Its lists are now bound by element, as above.
- **The targeted check after these fixes:**
  - **iPhone:** the save test passed (35 s). On the phone, the export picker is now pulled down from its own bar, not the status bar above it, and it closes.
  - **375 window: both tests still fail, so 7A is not met at 375.**
    - In a 375-point window the export picker fills the whole window. It has no Cancel and no sheet to pull down, only Save (never pressed) and a chevron (`n4p-ui-after-export.png`), so the test can't close it the way it does elsewhere. The next step is to find what closes a whole-window picker on iPadOS (the chevron, or a swipe from the window's edge) and try it there.
    - The warned-engine test failed there in all three of my runs. The Engine menu closes while the test scrolls it under the keyboard.

**B138** (on the phone in portrait, `testWindowsAndNames` opens a dropped wire's palette instead of entering the group) is not in the waits. It still fails with no wait at its limit, so it stays open as it was.

## For the phone, now that B134's configuration is on the dev branch

`PHONE=1 native/apple/PlumCraftPad/conformance.sh gestures` runs these waits as they are. The status file is read the same way on the simulator, and nothing else is needed.

## Unverified

Nothing ran on a device, where the fixed 300 ms still applies.
