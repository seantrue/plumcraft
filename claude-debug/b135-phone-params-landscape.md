# B135: the parameters beside the canvas on an iPhone in landscape

*2026-11-23, branch `fix/b135-phone-params-landscape` from `origin/feat/native-n0`, brought up to `ede5705` (#175, #176). Uncommitted. No device.*

## Answer first

On an iPhone turned to landscape, the parameters now sit in a column of readable width at the trailing edge, and the canvas stays live beside it. The column is 45% of the window, at least 320 and at most 420 points; 393 points on an iPhone 17 Pro.

Before, they were a full-screen sheet with every field and slider stretched across 874 points.

One rule decides it, in one place (`PlumCraftPad/App/ReadableColumn.swift`):

| Window | Presentation |
|---|---|
| height compact and wider than tall (an iPhone turned) | side column, canvas beside it |
| width regular (an iPad) | popover from the node, as before |
| width compact, height regular (an iPhone upright, a narrow iPad window) | sheet, as before |

Turning the phone moves the parameters between the column and the sheet. Anything being typed is set first, so nothing is lost (B129).

I walked every other presentation in landscape. None had B135's fault except one viewer's layout, which I filed rather than changed (B140, a decision for the maintainer), plus one different fault (B139).

## Before and after

At one step larger text, on the iPhone 17 Pro simulator, in landscape. The pictures are in `claude-debug/b135-pictures/`:

- `before-params-landscape.png`: a full-screen sheet; the label field runs the whole width.
- `after-params-landscape.png`: the column at the right, the graph at the left.
- `after-canvas-beside.png`: a face left of the column, tapped, is selected (the canvas takes taps beside it).
- `after-params-slider.png`: amount dragged by its slider to 1.00, the core's value in its field.
- `after-params-portrait.png`: turned upright with "turned" typed in the label: a sheet, and the value set.
- `after-params-landscape-again.png`: back to landscape, a column again.

Portrait was not broken, so it is unchanged: a sheet at half height with a grabber and Done.

The keyboard: in the column, the field being typed into stays above the keyboard (B120's lift). It is pictured in the walk's `n4s3-ui-count-typing`.

## The test

`testParametersBesideTheCanvasInLandscape` runs on the iPhone simulator; on an iPad, whose height is never compact, it skips. In landscape it:

1. Opens a node's parameters by a double tap, and checks that the app says column and that the column's readable width is at most half the window's and at least 300.
2. Taps a face beside the column, and the core selects it.
3. Drags the amount slider, and the value changes.
4. Types into the label and turns the phone upright: the sheet shows the value set.
5. Turns back to landscape: a column again.

It passed in 21 s.

## Every landscape presentation

These are on the iPhone 17 Pro at one step larger text in landscape (874 by 402), and on the iPad in a 375 by 655 window turned. The rule: controls keep a readable width and don't stretch across a wide window. Pictures: `walk-iphone-landscape.png`, `walk-iphone-landscape-windows.png` and `walk-ipad-375-turned.png` (contact sheets), from the UI tests run with `PLUMCRAFT_ORIENTATION=landscape`.

| Presentation | iPhone, landscape | iPad, 375 window turned | Verdict |
|---|---|---|---|
| Parameters (a node's) | was a full-width sheet; now the side column | sheet, the window's width | **fixed** (B135) |
| The inline row editor's fields, a choice's menu | on the face, the keyboard below with its bar | the same | fits |
| Value editors: the number and text keyboards | the field lifted above the keyboard | the same | fits |
| Pop-outs: picture, array, SVG, text, YAML | a full-screen sheet; the picture or the text takes the width, the bar's buttons at its ends | the window's width | fits: a viewer's width is the value's |
| Pop-out: audio | whole | its bar runs past both edges | different fault: **B139** |
| Pop-out: a table's records | names at the left edge, values at the right, about 800 points apart | the window's width | same fault, in a viewer: **B140**, a decision, not changed |
| View › Parameters… (the graph's) | full-screen sheet, centred text (the sample names none) | sheet | fits, with no rows to judge; a graph with named ports isn't pictured |
| Settings | form sheet, inset list | sheet | fits |
| Settings › Cache, Cache Report | form sheet | sheet | fits |
| Compare | not reached on the phone (see below) | sheet | fits on the iPad |
| Engine › Attach… (a form) | not reached on the phone (see below) | the system's alert | fits on the iPad (an alert is the system's width) |
| The Engine menu | not reached in landscape; pictured upright | menu | menus are the system's width |
| The More menu, a context menu | the system's menu, scrolled in the short height | the same | fits |
| Add Node | full-screen sheet on this branch; on B134's branch, a 340-point sheet at the bottom edge | sheet | fits once B134 is merged |
| The save's question, Recipe | not run: they need the test engine, which wasn't attached | not run | not seen |

On the phone in landscape, the compact-windows test stopped before the Engine menu: after the More menu was scrolled, its "Engine" item wasn't found. Compare and Attach come after that step, so they weren't reached. It is a test-side limit I didn't chase further: the menu does scroll there, pictured.

`testWindowsAndNames` passed on the phone in landscape, so B138 (B134's branch) is the portrait layout's alone.

## New entries

- **B139.** iPad, a 375-point window: the audio pop-out's bar runs past both edges.
- **B140.** iPhone, landscape: a table's pop-out spreads each record's names and values across the whole width. Open question: should a pop-out's lists keep to `ReadableColumn.width` while pictures keep the whole width?

## The branch

- `PlumCraftPad/App/ReadableColumn.swift` (new): the rule and the width.
- `PlumCraftPad/App/Parameters.swift`: the column embedded as a child of the graph's view controller, at the trailing edge and the full height. It is removed on close, and `relayout()` moves it between column and sheet. `describe` reports `style` (`column`), `frame`, `windowWidth` and `safeTrailing` for the tests.
- `PlumCraftPad/App/GraphViewController.swift`: a size-class change calls `relayout()`.
- `PlumCraftPad/UITests/GestureTests.swift`: the new test. The More menu helper also scrolls a menu in a short window, and the compact-windows test pictures Settings and scrolls its list there.
- `docs/BUGS.md`: B135 under Fixed, plus B139 and B140. `docs/STATUS.md`.

## Gate

The closing round after the merge with `ede5705` (`b135gate.sh`, 11 minutes). The merge was a fast-forward plus a three-way apply, with no commit; BUGS and STATUS conflicted, and both sides were kept:

| Check | Took | Result |
|---|---|---|
| xcframework | 61 s | built |
| iPad standard: device build, whole suite, UI tests | 326 s | 59 of 59; UI 7 passed, 2 skipped (the test engine's, and the landscape test, which needs a compact height), 0 failed; home listing unchanged |
| Every UI test on the iPhone (portrait, but for the new test) | 282 s | 7 passed, including `testParametersBesideTheCanvasInLandscape` (20 s); `testWindowsAndNames` failed, which is B138 (portrait, as on B134's branch) |

Before the merge:

| Check | Took | Result |
|---|---|---|
| `testParametersBesideTheCanvasInLandscape`, iPhone | 4 min with the build | passed (21 s) |
| `testInlineEdits`, `testWindowsAndNames`, iPad landscape, whole screen | 5 min | both passed, as without the change |
| the walk on the iPhone in landscape | 6 min | 4 passed; the compact-windows test stopped at the Engine menu (above) |
| the walk on the iPad, 375 window turned | 10 min | 3 of 3 passed |

## Unverified

Nothing ran on a device. The phone's xcodebuild twice failed to exit after its tests had finished (the results were already written), and I stopped it by its PID.
