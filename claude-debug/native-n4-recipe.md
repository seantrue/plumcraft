# Recipes only with lineage on the Apple shells, and B124 found

Branch `native/n4-recipe` from `feat/native-n0` (PR #167), 2026-11-22, uncommitted.

## Answer first

B124 was not a test. The file was written by an old trial build of the Mac app, from when the app was named PlumCraft (`com.plumcraft.mac`, built 2026-11-21, kept in the main checkout's `claude-debug/native-n3/s6-try/n4s6/PlumCraft.app`). LaunchServices launched it at 15:28:26, and the system log shows that process at 15:29:31.81, the second the file appeared. It is still running. It was not signalled, and the file was left in place.

The viewer bar on the Mac and iPad now offers Recipe only when `hasRecipe` is true. The Mac's person check now expects Recipe to be absent for the built-in engines' values.

## B124: how it was found

| Suspect | Run | Result |
|---|---|---|
| The Swift kit's tests | `HOME` and `CFFIXED_USER_HOME` set to a scratch folder | No preferences written. They did write `Library/Caches/plumcraft/open/export` (the export leak, which PR #167's `testing()` rule now keeps in scratch) |
| The iPad simulator's conformance | Run alone | The stray file's time and inode were unchanged |
| The iPad simulator's UI tests | Run alone | The stray file's time and inode were unchanged |
| The system log, 15:29:28 to 15:29:34 | Searched | Process `PlumCraft` (bundle `com.plumcraft.mac`): file coordination and a recent-documents update at 15:29:31.81 |
| That process | `ps` | Launched at 15:28:26 by LaunchServices on demand and brought to the front; its parent is launchd, not a script; its binary is the trial build named above |

That build kept its preferences in `Application Support/PlumCraft`; today's app uses `Plummer`. This was a launch of a real app with the person's Library, not a test leak, so there is no test to fix and no guard that would have stopped it.

What would prevent a repeat: trial builds left where LaunchServices registers them can be started as a document's app. Proposed: unregister them and remove them (the maintainer's decision; nothing was removed).

## Recipe

| Change | Where |
|---|---|
| `PopoutValue.hasRecipe` | `PlumCraftModels/Messages.swift` |
| The bar shows Recipe only when `hasRecipe` is true (Mac and iPad: one SwiftUI bar) | `PlumCraftShell/Viewers.swift` |
| Each pop-out's `hasRecipe` in the Mac's status | `PlumCraftMac/App/Popouts.swift` |
| Person check: "the pop-out's bar offers no Recipe for a built-in engine's value (no lineage)", using the app's own report and accessibility | `person-check.py` |

No string promises a recipe unconditionally. The core's save message says "with its recipe" only when the engine wrote one.

Should `crates/ports` supply `Display.recipe`? Yes, recommended: one test-engine node sending a recipe would let a launched check show the button present and its text. That is the half of F56 no app can show today, since none of their engines track lineage.

The F56 row is updated:

- Mac: partly met. Absence is shown; presence is not.
- iPad: not met.

## Proposal (not built): a saved value's graph name and hash

Put it in the save's outcome in the status bar: "Saved marked.quoted · from save-check (9f86d0…)", with the full hash on hover and in the log. Show it only when the engine says it stamped the file. Do not show it in the viewer or in anything that reads files back. A possible later step is Finder's Get Info, through an extended attribute.

## Gate

| Run | Result |
|---|---|
| xcframework (macOS, iOS, simulator) | rebuilt |
| `swift test` | passed |
| Mac gate (screen unlocked) | 64 of 64 cases, 941 steps; person check 54 passed, 0 failed, 1 seen (Open Recent's items); home and Library unchanged |
| iOS device build | built |
| iPad simulator conformance | 59 of 59 cases, 5 skipped |
| iPad UI tests | 7 of 7 passed |
| Private strings | no match |

The first Mac gate run had one failure: the new Recipe step read the pop-outs from the wrong part of the status. I fixed the step and ran the gate again in full; that run is the one in the table.

Floor counts are unchanged:

| | Met | Partly met | Not met |
|---|---|---|---|
| Mac | 51 | 3 | 31 |
| iPad | 38 | 1 | 46 |
