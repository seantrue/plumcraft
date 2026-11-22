# The engine half continued, and the audio engine compiled in (2026-11-22, branch `native/n4-engine-half`)

From `origin/feat/native-n0` at `deda565` (#157 merged). Nothing committed. The maintainer's answer: "Compile array,audio,text in. Yes to the rest"; the controller's reading, recorded in note 24: question 3 goes against the recommendation (the iPad's engine half uses engines compiled in, the audio engine joins text and array in process), and 1, 2 and 4 are the recommended options.

## What was built

| Item | State | What verified it |
|---|---|---|
| 1, audio compiled in | Done (the native apps); egui keeps it as a process (a decision) | A C ABI test (its demo in process on a platform with no programs); the conformance case `engine:audio` on the C ABI, its port, egui linked to it, the Mac app and the iPad simulator; the person check: started in process, the audio viewer, Play and Pause pressed through accessibility |
| 2, the iPad's engine half in process | Done | Two conformance cases on `text.slow` and `text.fail`: the core, the C ABI, its port, egui, the Mac app, the iPad simulator |
| 3, what is animating | Done; B111 still not reproduced | `animating` in both apps' status; the UI tests print it on every time-out; `testInlineEdits` asserts it empties within 8 s of the number's Done: passed with the software keyboard shown, status by file and by element |
| 4, the test engine | Done (Mac-only, a decision) | `crates/ports`; three conformance cases on egui and the launched Mac app; SDK sub-ports fixed in Rust (B115) and Python (B116, with variadic slots) |
| 5, the iPad's warning mark | Done | `testAWarnedEngineIsMarkedInTheMenu` on the simulator; a C ABI test on the iPad's platform |

## Floor counts

| Shell | Met | Partly | Not met | Before |
|---|---|---|---|---|
| Mac | 50 | 3 | 32 | 43, 2, 40 |
| iPad and iPhone | 38 | 1 | 46 | 28, 1, 56 |

Mac rows newly met: F13, F14, F15, F17, F18, F20, F21 (the test engine), F16 partly (the path widget and the engine's reason; the file panel is not opened, decision 2). iPad rows newly met: F28, F29, F30, F31, F34, F36, F42, F45, F46, F72, and F39's seven states. Not coverable on the iPad: F35 and F2 (a process killed and restarted). Still unmet on both: F19 (the Rust SDK can't declare save formats), F24 and F25 (not attempted), and the rest note 24 lists.

## Why the audio engine was a process only, and its size

Not a thread, a device or real-time output: the engine is offline DSP and decoding. Its SoundCraft crates are git dependencies, not on crates.io, and the core and the egui editor are published (commit fb9e074, "audio as a process"). The application now registers it (`engines::CompiledAudio`); the C ABI library, never published, links it. Static library 113.2 to 121.4 MB a slice (+8.2 MB); the xcframework 324 to 348 MB; the Mac app's binary 41.6 to 45.1 MB (+3.5 MB).

## What Windows must do for audio in process

Nothing but rebuild `plumcraft_ffi.dll` from this branch: the library links the audio engine and registers it at open, so the Windows app offers Audio in process like Text and Array. Its C# runner should claim `engine:audio` (the case "the audio engine compiled in runs a tone…") and, like the others, may claim `engine:ports` only with the test engine installed (it isn't, on Windows). The engine programs it ships beside the app are unchanged by this.

## Contract changes

- `docs/shell-interface.md` 4.2: the C ABI core offers the audio engine in process (its default), its process and HTTP hostings disabled with the reason where no programs start; egui keeps it as a process. No message changed shape.
- The conformance suite: new needs `engine:audio`, `engine:ports` and the step capability `ports_installed` (`crates/editor-core/src/conformance.rs`).
- Status files (scripts' and tests' reading, not the contract): `animating` in both Apple apps, `menuMarked` on the iPad.

## Bugs

- Fixed: B115 (the Rust SDK passed no choice sub-port), B116 (the Python SDK passed neither sub-ports nor variadic slots), B117 (the person check with the screen locked).
- Open, new: B118 (the iPad simulator's software keyboard up after a launch with nothing edited).
- Looked at again: B111 (not reproduced; the device re-run now says what was animating).

## Accessibility in the gate runs

Checked with `IOConsoleLocked` before and after each run. The three Mac gate runs that count (below) had the screen unlocked throughout, so every accessibility step ran (Recipe, the audio viewer's Play and Pause, the sheets' layout from outside, the save and open panels' own fields). Two earlier gate runs had it locked (the maintainer away): there the accessibility steps said SEE as designed, and the engine half's timing failed three steps (F29, F30, F36) because a locked Mac ran the app's steps more slowly; the slow node now takes 6 s and the Auto-Run check is waited for (B117's note). The iPad simulator runs don't use the Mac's accessibility.

## The gate

| Step | Result |
|---|---|
| `cargo fmt --all --check` | Clean |
| `cargo clippy --all-targets --all-features -D warnings` | Clean |
| `cargo nextest run --all-features --no-fail-fast` | 1,609 of 1,610; the one failure is the known `hidden_params_match_the_baseline` |
| Hash and core-toolkit gates | Both pass |
| Python (`uv run pytest`, ruff) | 235 of 235 (5 new); ruff clean |
| Private strings | Clean in 1,276 tracked files; the new untracked files checked against the same patterns, clean |
| Shell schema | Regenerated; unchanged |
| xcframework (macOS, iOS, simulator) | Rebuilt with the audio engine |
| `swift test` | 97 tests, 0 failed |
| `PlumCraftMac/conformance.sh gate`, three times, screen unlocked | Each: 63 of 63 cases (909 steps; the audio, engine-half and ports cases included), then the person check: 50 PASS, 0 FAIL, 1 SEE (Open Recent's items outside tracking); the person's Library and home listings unchanged |
| Mac Release, iOS Release (device SDK, not signed, not installed) | Both build |
| iPad simulator conformance (a simulator booted by its UDID, shut down after) | 59 of 59, 4 skipped (the three ports cases, Mac-only, and Install Engine…, which the iPad can't) |
| The iPad UI tests on that simulator, status by the element as the device reads it | 6 of 6 (the five, and the new warned-engine test) |
| egui's tests | In the full run (`plumcraft-ui`), all pass, the audio and ports cases included |

## Decisions for the maintainer

1. egui and audio in process: publish SoundCraft's two crates to crates.io so egui can link the audio engine, or egui keeps it as a process (kept so here).
2. The test engine on the iPad: a separate test xcframework with a test-only feature, or the ports rows stay Mac-only (kept Mac-only).
3. F19: add save formats (and a save operation) to the Rust SDK so a result node offers Save Result, or leave F19 unmet (left).
4. B118: look into the stray keyboard on the iPad (it may bear on B111), now or after the device re-run.
