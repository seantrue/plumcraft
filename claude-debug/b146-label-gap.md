# B146: a named wire's tags, one small gap from their pins

*2026-11-23, branch `fix/b146-label-gap` from `origin/feat/native-n0`, brought up to `2d0d106`. Uncommitted. No device.*

## Answer first

**Cause.** The two sides were already one rule. Both tags sit the core's stub from the pin's centre: a driver's tag starts 14 points right of its output pin, and a receiving tag ends 14 points left of its input pin (`NET_STUB` in the core's scene). Every shell draws them where the core says. In the maintainer's picture the two gaps measure the same, about 23 pixels each (12 points). The input side looked too large; it was no larger than the output side.

**Fix.** "Both": the one stub is now 8 points on both sides. That leaves a small gap beside a pin's square, about 4.5 points visible where it was about 10.5. How much smaller was not said; the controller's reading, the same small gap on both sides, is what was built.

The value is held in one place, `plumcraft_session::layout_cmds::NET_TAG_STUB`, and taken by:

- the core's scene (`plumcraft_editor_core::scene::NET_STUB`, `DEFAULT_USE_AT`), which the Mac, iPad and Windows shells draw by;
- the tidy layout's spacing in the session;
- the egui editor (`plumcraft_ui::net_view::STUB`).

A tag a person has moved keeps its own place.

## Pictures

From the iPad simulator, in landscape, Windows and Names at 120%. "said ×2" is beside Every widget's output; "said" is beside Reverse's input and Upper case 2's. The pictures are rotated as the simulator saves them, in `claude-debug/b146-pictures/`:

| | Default text | One step larger |
|---|---|---|
| Before (14) | `before-default.png` | `before-one-step-larger.png` |
| After (8) | `after-default.png` | `after-one-step-larger.png` |

The UI test that takes them is `testANamedWiresTagsBesideTheirPins` (`PlumCraftPad/UITests/NetTagTests.swift`).

## What moved

- `crates/session/src/layout_cmds.rs`: `NET_TAG_STUB` (8), and the layout's `TAG_STUB` now takes its value from it.
- `crates/editor-core/src/scene.rs`: `NET_STUB` comes from the session's constant.
- `crates/ui/src/net_view.rs`: `STUB` comes from the core's.
- `crates/ui/tests/kittest_net_labels.rs`: it checks against the constant, not 14.
- `crates/editor-core/fixtures/scene/full.json`: the shared fixture, rewritten (`PLUMCRAFT_WRITE_FIXTURES=1`). The net's driver is now `at: [8, 0]` and its use `[-8, 0]`.
- **Shells' tests.** The Swift model tests and the Windows `ModelsTests.cs` read that fixture but check nothing of the stub. A Swift canvas test aims 12 points into a driver's tag, which is still inside it at 8.
- **For the Windows agent: no Windows files change.** Run `PlumCraft.Kit.Tests`, whose `ModelsTests` read the fixture, and look at a named wire on the canvas: Windows draws the tags where the core's scene puts them.

## Gate

The final round on the merged branch (`2d0d106`), `claude-debug/n4-decided-2/b146final.sh`:

| Check | Result |
|---|---|
| Rust gate | clean except the known `hidden_params_match_the_baseline` (1675 of 1676) |
| swift test | passed |
| Mac gate twice (screen unlocked) | 71 of 71 cases each; person check 64 passed, 2 failed, 1 seen. The 2 failures are the curves editor's own steps (S4a: a drag on the curves plot, then its Undo) |
| The dev branch's own Mac gate, for comparison | 71 of 71; person check 62 passed, 0 failed, 10 seen. Its S4a drag was only "seen" (the screen was locked, so no drag reached the app) |
| iPad standard gate | 60 of 60; UI tests 12 passed, 4 skipped, 0 failed |

The S4a drag failure is the curves editor's, which this branch doesn't touch. On the dev branch the step never ran, so the comparison doesn't clear it either way: it is for that editor's owner to look at, with an unlocked screen.

Before the merge:

| Check | Result |
|---|---|
| Rust gate | clean except the known `hidden_params_match_the_baseline` |
| swift test | passed |
| iPad standard gate | 60 of 60; UI tests 10 passed, 4 skipped, 0 failed |
| Mac gate twice | 69 of 69 cases each. The person check failed only the curves editor's own steps (S4a: a drag on the curves plot, then its Undo) and a File › Close after them, which this branch doesn't touch; compared with the dev branch in the final round |

## Not checked

A device. Windows (above). The egui editor was checked by its tests only, not looked at.
