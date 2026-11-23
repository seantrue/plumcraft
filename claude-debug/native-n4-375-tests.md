# 7A finished: the 375 by 655 window at one step larger text

*2026-11-23, branch `native/n4-375-tests` from `origin/feat/native-n0` (`ee127ef`). Uncommitted. Time box 60 minutes; about 46 used.*

## Answer first

In the 375 by 655 window, turned, at one step larger text, one full pass of every iPad UI test passed: 10 passed, 1 skipped (the iPhone-landscape test, which needs a compact height), 0 failed. The two 7A tests pass there. All the faults were the tests'; there is no new app bug, so B144 stays free.

## The save test's export picker

In a 375-point window the export picker fills the window. Its bar has Browse (back), "…" (view options only), Save and Tags. There is no Cancel, Escape doesn't close it, and a pull-down doesn't either.

**A person can leave without saving:** Browse, then the ✕ at the top of the list of places, which accessibility calls "Cancel" (`n4p-ui-picker-browse.png`). The test now does the same: Escape first, then Browse › Cancel, then the pull as a last try. Save is never pressed.

Two more of the test's own faults were behind it:

- The recipe sheet's pull-down was placed by the screen's proportions, not the window's.
- The value's sheet was pulled instead of closed by its own Done (`popout-done`), as a person closes it.

## The warned-engine test

The keyboard is up because the simulator brings the software keyboard up with any open menu: a menu takes typed letters to jump to an item (B118, the stock Files app's menus too). The earlier steps edit no field: the test opens the More menu and then the Engine menu straight after launch.

On a device a person isn't expected to see the keyboard there, but no device run has checked it. The test can't type into the menu either: XCTest reports no element with the keyboard's focus, and the keyboard's own keys didn't bring the engine into sight. That step is still in the test, harmless and inert.

What made it pass is the earlier change of binding its lists to their elements, not to an index. It passed in all 3 runs here (3 of 3 failures before that change), and in the full pass.

## Runs

| Run | Result |
|---|---|
| Both tests, Escape and typing | warned engine: XCTest's typeText refused (no focus); save: picker still open |
| Both, keys tapped, "…" menu | warned engine passed; save: picker's "…" holds view options only |
| Save, Browse after "…" | Browse not reached |
| Save, Browse first | picker closed by Browse › Cancel; the recipe's sheet not pulled away |
| Both, pull within the window | warned engine passed; save: the value's sheet not closed |
| Save, the value's Done | passed (44 s) |
| Full pass, 375 turned, one step larger | 10 passed, 1 skipped, 0 failed |

Each two-test run took about 7 to 8 minutes, including the Settings switches to Windowed Apps and back. The full pass took about 12. xcodebuild twice failed to exit after its tests had finished, and I stopped it by its PID.
