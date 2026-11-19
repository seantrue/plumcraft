# N1 exit review: the editor core is cut

Date: 2026-11-19. Branch: `feat/native-n0` at `eea4740` (slices 1 to 8 merged; `main` untouched at `34f6aea`).

Stage N1 of `docs/research/24-native-shells.md` cut a toolkit-free core out from under the egui editor. This review checks it against the plan's exit line, lists what is not done, and says what N2 can start from.

## The exit line

The plan (section 12): "Every existing test and probe passes unchanged; the control protocol's replies are byte-for-byte what they were; no user-visible change."

| Criterion | Result | Evidence |
|---|---|---|
| Every existing test passes | Met, with four test edits | Local suite as CI runs it: 1,394 of 1,395 under b3 and md5, the one failure (`hidden_params_match_the_baseline`) there before N1. Linux CI on the last pull request (#106): every test passed. |
| Tests unchanged | Not quite | Four existing tests were edited: attach waits for the worker (slice 3); a viewer test waits for the media service (slice 6); two C ABI tests written during N1 were corrected (after slice 7). |
| Control replies byte for byte | Met for the egui editor | 41 replies in nine states compared before and after in slices 5, 7 and 8: no difference. `engine.pending` is now true during a switch (documented). |
| No user-visible change | Three small ones | Attach shows "Connecting…" in place of a frozen window; a failed app start reports in the status bar a moment later; a thumbnail or watched value can show a placeholder while it is read. |
| Probes pass unchanged | Met | The existing probes that start the editor and send at once still run. `ui.invoke` of a dialog command now replies at once with an ask (decided, question 1); no probe relied on the old blocking. |

## What the plan listed for N1

| Item | State | Note |
|---|---|---|
| `plumcraft-editor-core` | Done | 13,040 lines; toolkit gate in CI. |
| `plumcraft-media` | Done | 1,834 lines; decoders and the media service. Compare, spectrogram, PDF pages and mesh shading are not requests on the service yet. |
| The egui shell on top | Done | `crates/ui/src` went from 23,762 to 19,500 lines; what is left is drawing, egui glue and the canvas. |
| Engine calls off the UI thread | Done, with exceptions | Runs, cancels, releases, switches, startup and value reads are on workers; a test fails if the editor's thread calls an engine. Exceptions: an engine compiled in connects in place (3 ms); `doc.open` and `value.open` read in place. |
| `Waker` | Done | Slice 1. |
| The asks | Done, partly | File, folder, prompt, form, text, clipboard, share, open.external, screenshot. Not built: streamed saves (`commit`), stored grants, rename as a `text` ask. |
| The logging facade | Done | A ring, `log.tail`, `log.write`, `PLUMCRAFT_LOG`, a file over the C ABI. The platform logs (unified log, logcat) and Export Diagnostics are not built. |
| `headless.rs` off egui | Half | The commands are a core module that links no egui; the `plumcraft-editor` binary still links it, being one executable with the window. |
| The two-sessions audit | Not done as an audit | Two documents run side by side over the C ABI with a test (slice 7), and the packs registry (per process) and manifests list (per thread) are known shared state; nobody has walked every `static` and thread-local for a second document. |

Added beyond the plan's list: one control dispatcher for every shell and a control port a C ABI core opens itself (slice 5); a core `Document` (slice 7); the menu model (slice 8); platform `Capabilities` (slice 2).

## What a native shell can do today through the C ABI

Open a core without waiting; open several documents; run any command; read the graph; receive runs' events; answer asks; fetch thumbnails and pictures without waiting; fetch the menus; read and write the log; switch engines; be driven by the same probes over the control port.

## What it cannot do yet (N2's work)

The scene: node geometry, hit testing, selection and editing gestures, wire routing, group focus, net tags, slice labels and hover previews are still inside `crates/ui` (`canvas.rs`, `edit_view.rs`, `wire_route.rs`, `groups_view.rs`, `net_view.rs`, `slice_view.rs`). `ui.set` and `ui.audio` are refused over the C ABI.

## Open items carried out of N1

| Item | Where recorded |
|---|---|
| The hash gate fails on Linux CI, every run | B28, issue #102 |
| `mode2_python_kill_9_mid_run_recovers` fails some Linux runs (2 of 9) | B29, issue #103 |
| Peak memory about 10% higher with thumbnails since slice 6, cause unknown | Note 24 section 16 |
| The in-place spectrogram stalls the editor's thread 35 to 195 ms | Note 24 section 16 |
| An engine compiled in still connects on the UI thread | Note 24 section 16; needs a wait in two kittests |
| PlumImpress's editors can show "gone" briefly on an HTTP engine | Note 24 section 16 |
| `main`'s CI still fails the eight kittests of B27 | Fixed on the dev branch only; issue #89 open |
| No iOS or Android device has run the C ABI; JNI only on a host JVM | Note 24 section 15 |
| `docs/shell-interface.md` is still Draft 1 and behind the code | Each slice's report lists the sections to bring up to date |

## What went wrong in the process

- Local runs on the Mac passed slices that Linux CI then failed, three times: the engine-binary lookup (B27, a week old), an intermittent host test (B29), and two C ABI tests after slice 7. The controller merged the first two slices on "the same failures as `main`" without reading why `main` failed.
- The gate "no user-visible change" covered the egui editor only. The C ABI's behaviour changed in most slices, and one change (`run.watch` runs) went unreported until a test failed.
- One agent's probe wrote the maintainer's real editor preferences (slice 4).
- The plan was wrong about logging (there was none to preserve) and about where arrays become pictures (the engine SDK).
- Build output grew to 89 GB; free disk fell from 189 GB to 37 GB over the day.

## Recommendation before N2

1. Bring `docs/shell-interface.md` to Draft 2 from the slice reports, since N2's shells are written against it.
2. Give the C ABI its own "no change" gate: a recorded set of request and reply pairs, as the 41 control replies are for the egui editor.
3. Fix B28, so a green Linux run means green.
4. Then N2 (the scene, and the first macOS shell), which needs the maintainer's go-ahead.
