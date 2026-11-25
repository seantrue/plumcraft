# Save Result on the Apple shells, and the person check under load

Branch `native/n4-save` from `feat/native-n0` (PR #165), 2026-11-22, uncommitted.

## Answer first

Save Result now works in every native shell with no shell code (B122):

- A tap on a face's Save Result… footer line is a core gesture.
- A menu item carrying `ask: true` that a shell runs with `command.run` is invoked by the core.

On the Mac it was launched end to end, and F19 is met for the Mac. The iPad's Save Result is written, but it can't be launched: no engine the iPad runs has a result node.

The flaky person-check steps (B87, F29, F36, and F35 with them) now wait on state, never on a time (B123). The Mac gate passed five times in a row, two of the runs under full CPU load.

The gate is complete. One thing appeared outside scratch while the iPad UI tests ran, with the cause not found: `~/Library/Application Support/PlumCraft/prefs.json`, holding default preferences. It was not deleted (below).

## What changed

| Area | Change |
|---|---|
| Core (`gesture.rs`, `geometry.rs`, `control.rs`) | A footer tap is decided by the line's kind (`FooterTap`). The `save` line's tap invokes `run.save {node, port, sheet, ask: true}`, with Contact Sheet… on the line's right half. `command.run run.save {…, ask: true}` is invoked too. `geometry.locate` takes `part: "save"`. |
| C ABI (`ffi/src/core.rs`) | A shell's own `ui.gesture`, and a `command.run` that asks first, belong to the person, so the shell shows the asks. Over the control port they stay a script's. |
| Text engine (Rust and Python) | New node `text.gate`: it passes its text once a file exists and stops on cancel. Tests both ways; the Python golden regenerated. |
| Mac (`Asks.swift`) | The save panel has a Format pop-up (`FormatChoice`). A format the system doesn't know leaves the panel unlimited, which fixes `Result.quoted.quoted`. A refused form shows again with its reason. A sheet waits until the window is free. The status gains `asking`. |
| Mac (`GraphDocument.swift`) | B87: the manifest problems are read from the engine's state and said in the banner. The status keeps every banner (`banners`). |
| Apple canvas | Reports its footer's lines as `parts.footer`. |
| iPad (`Asks.swift`) | Save Result… is an alert with the name and the formats, then the document picker's export. A refused form shows again. |
| Swift runner | Each case gets a `$scratch` folder (`--scratch`). The Mac claims `saves`, and an app that measures its own faces claims `footer`. |
| Conformance case | Runs the menu item's way (`command.run`). Where the footer is measured, it taps the Save Result… line and sees `file.save` asked. |
| Person check | F36, F29 and F35 wait at the gate and on the faces' state. B87 reads the banners. The F19 steps open the real panel, read it and cancel it (never pressed). They then answer the panel's ask by script, set the options in the app's own sheet and read the file back. |

## The Mac gate, five times

| Run | Load | Screen locked | Suite | Person check |
|---|---|---|---|---|
| 1 | none | no | 64 of 64 cases, 941 steps | 54 passed, 0 failed, 1 seen |
| 2 | 14 busy loops (one per core) | no | 64 of 64 | 54 passed, 0 failed, 1 seen |
| 3 | none | no | 64 of 64 | 54 passed, 0 failed, 1 seen |
| 4 | 14 busy loops | no | 64 of 64 | 54 passed, 0 failed, 1 seen |
| 5 | none | no | 64 of 64 | 54 passed, 0 failed, 1 seen |

In every run the one "seen" line was Open Recent's items, as before, and the home and Library listings were unchanged.

## Pictures

- `save-panel.png` (run 1, screen unlocked) shows the real save panel on the graph window: "Save Result", Save As `Result.quoted`, Where the scratch folder, and Format "Quoted Text (.quoted)" under it.
- `save-options.png` shows the app's own options sheet: "Save as Quoted Text", a Quote mark pop-up, Cancel and Save.

## The rest of the gate

| Run | Result |
|---|---|
| iOS device build (`build-for-testing`, generic iOS, no signing, nothing installed) | built |
| iPad simulator conformance | 59 of 59 cases, 5 skipped |
| iPad UI tests (simulator, Full Screen Apps) | 7 of 7 passed |
| Rust and Python gate | schema, fmt, clippy, hash and core clean. nextest 1,636 of 1,637: the one failure is the known `params_coverage hidden_params_match_the_baseline`. pytest and ruff clean. |
| `swift test` | passed |
| xcframework (macOS, iOS, simulator) | rebuilt |
| Private strings | no match |

## Outside scratch: not explained

`~/Library/Application Support/PlumCraft/prefs.json` appeared at 15:29:31. That falls inside the iPad part of the gate (15:27 to 15:32, while the UI tests ran), and the home listing taken around that part shows it. It holds the core's default preferences (no theme, no folders, engines picked automatically).

The name points to a process that used the core's own default, `path_for("PlumCraft")`, which only `PLUMCRAFT_PREFS` moves (the scripts set `PLUMCRAFT_EDITOR_PREFS`). Nothing found ties it to a step of this gate:

- the iPad app puts its preferences in its container;
- the runner has no core;
- the five Mac runs, the same day, left the listings unchanged.

It may be this gate's or another process's on the Mac at that minute. It was left in place for the maintainer to decide on.

## Floor counts

| Floor | Met | Partly met | Not met |
|---|---|---|---|
| Mac | 51 | 3 | 31 |
| iPad | 38 | 1 | 46 |

F19 is newly met on the Mac. The iPad is unchanged.

## For Windows

With the core doing the footer tap, the shell-side dispatch becomes redundant: the click on the Save Result… line in `CanvasView.cs`, and the `ask: true` special case in `DocumentModel.ActivateAsync`. Both can go once the Windows canvas reports its footer's lines in `parts.footer`. Whether it already does, I haven't checked.
