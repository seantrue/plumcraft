# Bugs

Bugs reported in passing (`bug: …`), kept until fixed. Numbers are never reused. Entries with an issue are also tracked on GitHub.

## Open

### B37. `fast_never_session_too_big_and_setup_bound_results_are_not_persisted` failed once on the Linux test host

- Reported: 2026-11-20
- Report: "record bugs"
- Context: `plumcraft-engine::persist`. Failed once in a full parallel run while three other builds were running on the host; the message was not captured. Passed 3 of 3 alone. Not looked into.
- Seen again: 2026-11-20, in the full run for N3 slice 2 with nothing else building. It fails at `crates/engine/tests/persist.rs:166`, the "too big" case: a 60 ms node that makes 32 MB was persisted, where the test expects it not to be worth it. Passed 40 of 40 alone. The test's own comment says the margin is a matter of timing.
- Seen again: 2026-11-20, on the Mac in N4 slice 2's gate, in the Python suite's test of the same name (`python/tests/test_persist.py`) while cargo was running in parallel. Passed 10 of 10 alone.
- Seen again: 2026-11-20, on the Linux test host in N4 slice 2's full run (the Rust test), together with B38. Both passed 10 of 10 alone afterwards.
- Issue: none

### B38. `memo_reuses_unchanged_nodes` failed once on the Linux test host

- Reported: 2026-11-20
- Report: "record bugs"
- Context: `plumcraft-text::conformance mode2_process`, `crates/text/tests/conformance.rs:390`, "off really runs the slow node". Failed once in a full parallel run while three other builds were running on the host. Not looked into.
- Seen again: 2026-11-20, on the Linux test host in N4 slice 2's full run, together with B37. Passed 10 of 10 alone afterwards.
- Issue: none

### B39. `bytes_held_elsewhere_are_shared_not_read_again` failed once on the Linux test host

- Reported: 2026-11-20
- Report: recorded by the controller under the maintainer's "record bugs"
- Context: `plumcraft-media` `service::tests`, `crates/media/src/service.rs:1133`, "let go of everywhere: read again": after the test dropped its two handles on the bytes, the service still answered with them. Failed once in the full run for N3 slice 2; passed 300 of 300 alone. Not looked into; whether a worker can still hold the bytes at that moment is the first thing to check.
- Issue: none

### B40. `scrolling_in_past_the_picture_asks_for_detail_once` failed once on the Mac under the full parallel run

- Reported: 2026-11-20
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: `plumcraft-ui::kittest_viewer_tools`. Failed once in the full parallel nextest run for N4 slice 2 on the Mac; passed 20 of 20 alone. The message was not captured. Not looked into.
- Issue: none

## Fixed

### B48. `build-xcframework.sh` ignored `CARGO_TARGET_DIR`

- Reported: 2026-11-21
- Report: found while preparing the Apple builds
- Context: the script read the libraries back from the checkout's own `target/` while cargo, honouring `CARGO_TARGET_DIR`, built them elsewhere; in a git worktree pointed at the main checkout's target directory it would have packaged stale libraries, or failed, rather than the ones just built.
- Fixed: 2026-11-21, Apple prep: the script reads from `CARGO_TARGET_DIR` when it is set. Verified by building the xcframework from a worktree into the main checkout's target directory.

### B47. A Mac app built for distribution would carry no engine programs

- Reported: 2026-11-21
- Report: found while preparing the Apple builds (the maintainer's standing instruction to record bugs found on the way)
- Context: the Mac app's Release bundle held only `PlumCraft`. The core looks for engine programs beside the app's own (`Contents/MacOS`), or in the cargo build a test runs from, so on another Mac the audio engine (never in process) and every engine hosted as a process (Engine › Rust · process) would have failed with "no engine binary". In-process text and array, which the suite uses, worked, so nothing caught it.
- Fixed: 2026-11-21, Apple prep (branch `native/n4-apple-prep`): `native/apple/release.sh mac` builds the text, array, audio and craft engines for both Macs and the Mac target's Bundle engines phase puts them in `Contents/MacOS`, each signed with the hardened runtime before the app seals them. Verified on the exported Developer ID build: the text engine switched to a process, audio and array added as processes, a graph run, the three children running from the bundle with the runtime flag. A plain Xcode build still has none (it runs engines from the cargo build, as before).

### B44. The iPad UI tests fail on a second run in the same app container

- Reported: 2026-11-21
- Report: found while getting the device gate ready (the maintainer's standing instruction to record bugs found in gate runs)
- Context: `testInlineEdits` and `testWindowsAndNames` timed out ("its undo name", "count set while hidden") whenever the app had run them before without being reinstalled, reading the status from the file or from the new status element alike; on a device, where the container stays, they would fail every time after the first.
- Cause: every document the iPad app opens autosaves (N4 slice 4), and a scripted launch's `--open-sample` opened the copy in Documents/Samples, so a test's edits were saved into the sample the next run opened, and into the person's samples.
- Fixed: 2026-11-21, the N4 device gate: with `--scratch-state`, `--open-sample` opens a fresh copy of the bundled sample in scratch (`Samples.scratchCopy`). Verified in the iPad simulator by running the five UI tests three times in one container (file, element, file). Still for the device: `device.sh ui`.

### B42. Exporting a value fails on a physical iPad: the export folder can't be written

- Reported: 2026-11-21
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: the conformance suite on the maintainer's iPad (debug build of the dev branch at `9b90500`), case "an array popped out by a person is a picture, and its numbers a grid paged by the core", step 13: `value.export {popout}` answers `<app container>/.cache/plumcraft/open/export/<pid>-<n>: Operation not permitted (os error 1)`. The core writes exports under `.cache` at the container's root, which an iOS app may not write; the simulator allows it, so the case passes there. Share, Save As and Drag Out use the same file, so they are expected to fail on the device too (not tried by hand).
- Issue: none
- Cause: the C ABI core never took the shell's `dirs.caches` (nor `dirs.scratch`): copies for apps and exported values went under the session's default, `HOME/.cache/plumcraft/open` on iOS (the desktop rule, `XDG_CACHE_HOME` or `~/.cache`, since iOS isn't `macos` to `cfg!`). On iOS `HOME` is the app's container, whose root a device app may not write; the simulator allows it, so every test there passed.
- Fixed: 2026-11-21, the N4 device gate (branch `native/n4-device-gate`): the core takes `dirs.caches` and `dirs.scratch` from the config (`plumcraft_ffi::core::Config::{caches, scratch}`): copies and exports go to `caches/open`, autosave's scratch files to `scratch/autosave`; the defaults that derive from `HOME` (open copies, the craft root, the HTTP host's CAS, preferences) use `Library/…` on iOS as on macOS. Found on the way: a short text (carried in its excerpt, with neither `value` nor `full`) exported and copied as nothing; fixed. Tests: `plumcraft-ffi::dirs` runs itself again with `HOME` read-only and every `dirs` root writable and takes values out every way the apps do (export, Share, the graph's Share, autosave in scratch and in the document's file, the open-value cache's root); it fails without the fix ("copies for apps go to …/home/Library/Caches/plumcraft/open"). In the iPad simulator with its data container's root made read-only (`chmod 555`, as a device has it): slice 6's build failed the same step as the iPad ("…/.cache/plumcraft/open/export…"), this branch's passed the whole suite, 52 of 52 (`claude-debug/native-n4/s7-b42-sim.sh`). Save As goes through the engine to the path the shell's ask gave (Documents) and the compiled-in engines save no formats, so it was not taken. Still for the device: the suite's export step on the iPad.

### B43. The graph's Parameters window is not reported open on a physical iPad

- Reported: 2026-11-21
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: the same run, case "the Cache Report and the graph's Parameters window are the core's: opened, read, edited and closed", step 11: `scene.get` has no `view.windows.params` where the case wants `true`. Passes on the Mac and in both simulators. One run only; whether it is timing on the slower debug build or a real difference is not looked into.
- Issue: none
- Cause: the iPad shell's, not the core's or the case's. Sheets the core opens and closes arrive faster than UIKit's transitions run (a script, the suite, a slow device). The Messages window (the case before this one in the suite) was dismissed while its presentation was still running, and the Cache Report was then presented over it; when the Messages sheet went it took the Cache Report and the Parameters sheet over that with it, and the shell, seeing them gone, told the core both were closed. Reproduced in the iPad simulator, launched as `device.sh` launches the app (Debug, `--scratch-state`): Messages opened and closed, then the Cache Report and Parameters opened at once, Parameters closed by itself within 0.11 s in 6 of 20 rounds (`claude-debug/native-n4/s7-b43-probe.py --messages`); the case alone looped 30 times never failed, which is why it passed everywhere but the device.
- Fixed: 2026-11-21, the N4 device gate: every presentation change the core drives on the iPad (core windows, Messages, Settings, a value's sheet replaced or closed) goes through one queue, `Presentations.whenSettled`, one at a time, in order, once nothing presented is mid-transition; a core window counts as closed by something else only after its own presentation has run. After it: 0 of 20 rounds of the probe, the case 30 of 30. New conformance case "windows opened as another closes stay open: the shell never closes what the core opened" with a new step form, `holds` (a condition that must stay so `for` a time, in both runners); it did not fail on slice 6's build in its one run there, so it catches the defect only some of the time. Still for the device: the suite on the iPad.

### B41. A run setting kept in the preferences is sometimes lost on the Linux test host

- Reported: 2026-11-20
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: `plumcraft-ffi::settings_autosave` `settings_are_the_cores_kept_in_its_prefs_and_every_document_follows_them`, `crates/ffi/tests/settings_autosave.rs:84`, added in N4 slice 4. After `theme.setToken`, `view.animationSpeed {ms: 1000}` and `lifecycle.appearance {dark: true}`, `settings.get {fromDisk: true}` gives Animation speed 500 (the default), not 1000. On the Linux test host it failed 1 of 10 alone at the dev branch before slice 5 and 6 of 10 at slice 5, which did not touch the settings code; it has not failed on the Mac. It may be the test's timing or a setting really lost when a document follows the shared look (`settings::follow` sets the animation speed from the preferences without a guard, unlike the run colours).
- Issue: none
- Cause: a defect in the editor, not in the test. `settings::after_command` wrote all three run settings (run colours, animation speed, Pick Engines Automatically) from whichever document ran a run command. Every document runs `engine.auto` when its first engine connects; a second document that connected after the first had set the speed, but before it had followed the shared look, wrote its own stale value back. A person with two windows could lose the setting the same way.
- Fixed: 2026-11-21, N4 slice 6: only the setting the command changed is kept (`Look::keep_run_pref`), and `settings.get {fromDisk}` reports the file's run settings. Test: `settings::tests::a_run_setting_changed_in_one_document_survives_another_that_has_not_followed_yet`. On the Linux test host the C ABI test file passed 100 of 100 runs at `e8a051b` (6 of 10 failed before).

### B36. A short clip's spectrogram is computed on the editor's thread

- Reported: 2026-11-20
- Report: "record bugs" (found while fixing B32)
- Context: `AudioView::sync` computes the spectrogram in place for clips under `SPEC_SYNC_SAMPLES`. A frame that shows a watched audio value took about 80 ms alone and 74 to 319 ms under load on the Linux test host (median 122 ms). It is not a wait for an engine, so B32's test no longer flags it.
- Issue: none
- Fixed: 2026-11-20, PR #131 (N4 slice 5): the spectrogram is computed on the media workers.

### B34. `a_save_in_the_app_feeds_the_nodes_downstream_and_runs` fails under the Linux test host's full parallel run

- Reported: 2026-11-20
- Report: "record the others"
- Context: `plumcraft-session` `doc_cmds::tests`: a poll returned `None` where the test expected the run's result. Failed in the baseline run of the dev branch at `b4a561b`; passes alone.
- Issue: none
- Cause (2026-11-20, confirmed): the test's saves came too close together for the file system's clock. Its last two saves (`{"doc": 1}` and then `{"doc": 4}`) are the same size as the save before each, and come 0.4 to 0.8 ms after it. The copy's clock on the Linux test host ticks once a millisecond. So often neither the size nor the modification time changed, and `poll_doc_edits` never saw the save. That is lines 784 and 791. An instrumented run printed an unchanged stamp before every failure. It is not load: on that host the test failed 91 of 100 runs alone, one at a time, and 123 of 200 with 40 copies at once. It passes on the Mac. That APFS's finer timestamps are the reason is inferred; it was not re-run there. Like B13, this is a test that met a clock. The session drops nothing in real use: a copy holds still for one 500 ms poll before it is read, so a later save lands at least 500 ms after the change the session last saw. Only a file system whose clock ticks more slowly than that (FAT, 2 s) could hide one. The test's polls run on a made-up clock, well under a millisecond apart.
- Fix: the test saves through a helper that writes the copy and, while its size and time are unchanged, waits a millisecond and writes it again, for at most 5 s. `doc_edits.rs` now states the limit. After: 0 of 100 alone and 0 of 200 with 40 copies at once.
- Fixed: 2026-11-20, PR #120

### B33. `hostile_files_never_panic_a_decoder` fails under the Linux test host's full parallel run

- Reported: 2026-11-20
- Report: "record the others"
- Context: `plumcraft-media::hostile`, `crates/media/tests/hostile.rs:98`: a decode went past the test's 5 s limit. Failed in the baseline run of the dev branch at `b4a561b`; passes alone.
- Issue: none
- Cause (2026-11-20, confirmed): the test, not a decoder. One input is slow. It is the empty SVG that claims `width='1e30'`, drawn by `svg::rasterise(&f, 1e9)`. The clamp holds: it is drawn at 4096 × 4096 (`MAX_RENDER_PIXELS`), not at its claim. Drawing those 16.8 million pixels and making them opaque takes 4.1 s in the test profile, against 0.23 s in release, so a 5 s limit on each file fails under load. Each file was timed alone through each decoder: every other input takes under 1 ms, apart from the first SVG parse, which loads the system fonts once (0.3 s). With 40 copies at once on the Linux test host it failed 37 of 200.
- Fix: the picture decoders stay timed against 5 s, which they reach only by believing a header. Each SVG raster is checked against the clamp that bounds its work (each side at most 4096, at most `MAX_RENDER_PIXELS`, a buffer of exactly w × h × 4) instead of being timed. After: 0 of 200 under the same load.
- Fixed: 2026-11-20, PR #119
### B32. `values_never_wait_for_an_engine` fails under the Linux test host's full parallel run

- Reported: 2026-11-20
- Report: "record the others"
- Context: `plumcraft-ui::engine_thread`, `crates/ui/tests/engine_thread.rs:546`: a spectrogram took longer than the test's 150 ms limit. Failed in the baseline run of the dev branch at `b4a561b` (80 cores, all tests in parallel); passes alone.
- Issue: none
- Cause (2026-11-20, confirmed): the test, not the editor. It held every frame under half of a 300 ms `resolve`, which measured the editor's own work in a frame, not a wait. With 40 copies at once on the Linux test host it failed 34 of 200: 32 in "watch n5" (the audio value; frames of 74 to 319 ms, median 122 ms), whose frame computes the Watched panel's spectrogram in place (`AudioView::sync`, clips under `SPEC_SYNC_SAMPLES`), and one each in the pop-out and compare phases. In all 200 runs no `resolve` was made on the editor's thread. Alone the audio frame is about 80 ms.
- Fix: the test holds each `resolve` at a gate until the editor has drawn three frames with it held, and fails if the gate has to give up on one (10 s): a frame that waited for an engine, directly or through a lock or channel, never returns while the call is held, however loaded the machine. Frame times are printed, not asserted. A deliberate fault (the Watched panel spinning until its value arrives) fails it. After: 0 of 200 under the same load, with audio frames up to 225 ms. The in-place spectrogram is still a stall of the editor's thread; moving it off the thread is a separate change.
- Fixed: 2026-11-20, PR #118
### B35. A blocking read's result could turn pending again (`blocking_reads_in_place_and_shares_the_cache` failed now and then)

- Reported: 2026-11-20
- Report: found by the B30 fix's full run on the Linux test host (failed at `crates/media/src/service.rs:1055`, "then cached"); 1 in 10 when run alone there.
- Cause (2026-11-20, confirmed): `MediaService::blocking` first calls `get`, which queues the request for a worker, then makes the value itself and keeps it as done, with the key still in the queue. A worker that reached the key after that set the done entry back to working and made it again, so a `get` meanwhile answered pending. Nothing else turns a done entry back. Not B31's cause (B31's test read too early); found while reading the same path. Reproduced 1 in 1,200 under load (0 in 300 alone); 0 in 1,200 after.
- Fix: a worker skips a queued request that is no longer queued (`run_one`). `a_queued_request_settled_in_place_is_not_made_again` fails without it (3 reads, want 2).
- Fixed: 2026-11-20, PR #117
- Issue: none

### B31. `a_request_answers_at_once_and_arrives_on_a_worker_with_a_wake` fails now and then on the Linux test host

- Reported: 2026-11-20
- Report: "record the others" (the four tests that fail only under the Linux host's full parallel run)
- Context: `plumcraft-media` `service::tests`. Failed once in the full run for N2 slice 2 (`85695df`) at `crates/media/src/service.rs:909`, "the shell was woken": the picture had arrived and the wake count was still zero. Passed 30 of 30 when run alone on the same host. The test is from N1 slice 6.
- Cause (2026-11-20, confirmed): the test read the wake counter too early. A worker keeps its result under the service's lock, lets the lock go, then calls the wake; the test asks without waiting for the wake, so a worker descheduled between the two lets it see the picture with the count still zero. That order (kept, then woken) is the right one: whoever a wake sends to ask again finds the result. Reproduced 112 times in 1,200 (60 copies at once, 20 rounds, with 40 busy loops alongside). Reading the path also found a real lost wake: when `MediaService::blocking` (`doc.open`, `value.open`) settled a request a worker was already making, the worker found it settled and neither kept its result nor woke, so a shell that had been told "pending" never heard; and a queued request settled that way was made a second time when a worker reached it.
- Fix: the test waits for the wake (10 s limit) and says what order is guaranteed; `blocking` wakes when it settles a request a worker had, after keeping it; a worker skips a queued request already settled. Three new tests: a wake always finds its result (200 rounds, the wake itself asks), and two for the in-place cases, which fail without the fix. After: 0 in 1,200 on the same load.
- Issue: none
- Fixed: 2026-11-20, PR #117
### B30. Video to frames fails with ffmpeg older than 5.1 (Ubuntu 22.04)

- Reported: 2026-11-20
- Report: "Bug:" (in answer to: the array engine's video reader passes `-fps_mode passthrough`, which ffmpeg only has from 5.1; Ubuntu 22.04 packages 4.4.2)
- Context: `video_goes_to_frames_and_back` (`crates/array/tests/channels.rs`) fails on the Linux test host with "video: /usr/bin/ffmpeg failed: Error splitting the argument list: Option not found". The option is passed at `crates/array/src/channels/video.rs:248`. GitHub's runner has a newer ffmpeg, so CI doesn't show it.
- Issue: #113
- Cause (2026-11-20, confirmed): `-fps_mode` was added in ffmpeg 5.1; before that the same setting is `-vsync passthrough`. It is the only option the video nodes pass that 4.4 lacks: on ffmpeg 4.4.2 the reader with `-vsync` (rgb24 and grayf32le), the ffprobe call, and the MP4 (libx264), MOV (prores_ks, profile 3, yuv422p10le) and GIF (palettegen/paletteuse, `-loop 0`, rgb and gray) writers all ran, and each written clip read back.
- Fix: the reader asks `ffmpeg -version` once per ffmpeg program and passes `-vsync` for a version before 5.1, else `-fps_mode`; an answer with no version in it (a git snapshot, or anything unparseable) gets `-fps_mode`. Test: `frames_pass_through_with_the_option_this_ffmpeg_has`.
- Fixed: 2026-11-20, PR #116

### B28. The hash gate fails on `main`'s Linux CI: "2 runtime commonkit v0.2 lines, want 1"

- Reported: 2026-11-19
- Report: "record the two main CI failures as bugs"
- Context: the `gate` job's "hash gates" step (`scripts/check-hash-gates.sh`) fails on `main` (latest `34f6aea`, run https://github.com/seantrue/plumcraft/actions/runs/43405581253) and on the native dev branch's pull requests, printing `hash gate: 2 runtime commonkit v0.2 lines, want 1; other versions: none`. The same script prints `hash gates: ok` on the Mac, on `main`'s code and on the dev branch's.
- Checked since (2026-11-19): on the Mac the script also passes under Rust 1.100.0, and `cargo tree … -i commonkit --target x86_64-unknown-linux-gnu` prints the same lines as for the Mac. So neither the Rust version alone nor the target's dependencies explains it. The runner's own `cargo tree` output has not been captured.
- Issue: #102
- Cause (2026-11-19, confirmed): CI sets `CARGO_TERM_COLOR=always`, so `cargo tree` wraps its `[build-dependencies]` heading in colour codes, and the script's `sed`, which cuts the output at that heading, no longer matches it: both `commonkit` lines are counted. Nothing to do with Linux or the Rust version; `CARGO_TERM_COLOR=always scripts/check-hash-gates.sh` fails the same way on a Mac. Found by making the script print the tree it counted and reading the runner's log.
- Fix: the script sets `CARGO_TERM_COLOR=never`, and prints the tree when the count is wrong.
- Fixed: 2026-11-19, on the native dev branch only, PR #109.

### B29. `mode2_python_kill_9_mid_run_recovers` fails on Linux CI: "the engine is not connected"

- Reported: 2026-11-19 (found by the controller on the native dev branch's pull request #101; not reported by the maintainer)
- Report: `plumcraft-text::conformance mode2_python_kill_9_mid_run_recovers` panicked at `crates/text/tests/conformance.rs:649` with `run: unavailable (-32006): the engine is not connected`, after 0.855 s, on run https://github.com/seantrue/plumcraft/actions/runs/44615077606 and again on its rerun: 2 of 2. Everything else passed (1,342 of 1,343). The Rust engine's twin, `mode2_kill_9_mid_run_recovers`, passed both times.
- Context: the same test passed on the four Linux runs before it that day (#98, #99, and #100 twice). The pull request's changes (N1 slices 4 and 5) touch nothing in `crates/text`, `crates/host`, `crates/conform`, `crates/proto` or `python/`. Not reproduced on the Mac: 40 runs alone and 48 runs twelve at a time all pass.
- Since (2026-11-19): intermittent, not constant. The Linux run of pull request #104 (the same code plus N1 slice 6) passed it, under md5 and b3: 2 failures in 7 Linux runs that day. #101 was merged into the dev branch with it failing (the maintainer's CPM, given to the slice 6 agent).
- Suspect, read from the code and not confirmed: in `crates/host/src/process.rs` the reader thread's end fails the run in flight before the supervisor has set the state to `EngineDown` (it reaps the process first, holding the `child` lock). `plumcraft_conform::kill_9_mid_run_recovers` gets the failed reply, then calls `wait_ready`, which returns at once on the stale `Connected`; `pid()` waits for the reap and is then not the first pid; the next `run` finds no writer.
- Open question: fix the host (state goes down before replies fail) or make the check wait for a new connection? The first changes what an editor sees after a crash, for the better if the suspicion is right.
- Issue: #103
- Cause (2026-11-19, confirmed; the suspect above had the order wrong): in `crates/host/src/process.rs` the supervisor failed the pending calls and live runs first and set the state to `EngineDown` after. Between the two, the check got its failed reply, `wait_ready` returned at once on the dead connection's `Connected`, `pid()` was `None` (already reaped, so "not the first pid" held), and the next `run` found no writer. A unit test that reads the state on hearing of the failure fails every time with that order.
- Fix: the state goes down first (`Inner::went_down`). An editor now sees the engine-state event before the failed run's reply, not after.
- Fixed: 2026-11-19, on the native dev branch only, PR pending. Not shown on Linux beyond CI passing: the failure was 2 runs in 9.

### B27. Eight editor kittests fail on `main`'s Linux CI, as in closed issue #89

- Reported: 2026-11-19
- Report: "record the two main CI failures as bugs"
- Context: found while comparing the native dev branch's pull requests (#98, #99) with `main`. The `gate` job's "cargo nextest, under md5 and then b3" step fails on every recent run of `main` (latest `34f6aea`, run https://github.com/seantrue/plumcraft/actions/runs/43405581253) with the same eight tests of `apps/plumcraft-editor/src/lib.rs`. `tests::audio_kittest`: `a_1khz_tone_peaks_in_its_spectrogram_bin`, `play_without_an_output_says_so_and_does_not_panic`, `the_audio_demo_runs_with_waveforms_and_an_audio_view`. `tests::engine_kittest`: `a_pick_adds_or_changes_an_engine_and_clear_starts_over`, `a_switch_cancels_the_run_in_flight`, `attach_connects_to_a_running_http_engine`, `switching_text_engines_runs_after_every_switch`, `text_to_audio_keeps_the_graph_and_reports_its_problems`. They pass on the Mac. Issue #89 ("CI gate: 8 editor kittests can't find the engine binaries (no engine binary at …/out/plumcraft-text-engine)") was closed as completed on 2026-11-12; the same eight still fail. The panic messages of the latest run have not been read against #89's, so that it is the same failure and not only the same tests is not confirmed. On the native dev branch the lookup (`engine_binary`) has moved to `crates/editor-core/src/engines.rs`.
- Found since (2026-11-19): it is #89's failure. `main`'s run panics with `no engine binary at /home/runner/work/plumcraft/plumcraft/target/debug/build/plumcraft-editor/62bf2a2969a8c725/out/plumcraft-text-engine` (and `…/plumcraft-audio-engine`). The test program on CI is at `target/debug/build/<package>/<hash>/out/`, not `target/debug/deps/`; `engine_binary` looks beside the program and, when the program is in `deps`, one folder up, so it finds nothing. CI installs the newest stable Rust, 1.100.0 (2026-11-09) on these runs; the Mac has cargo 1.99.0 and builds test programs in `target/debug/deps/`. That 1.100 moved them is inferred from those two facts, not read from its release notes.
- Also hit by: `plumcraft-ffi::switch starting_a_process_engine_never_holds_the_core_thread`, new in the native dev branch's pull request #100, which fails on Linux CI with the same message (`…/build/plumcraft-ffi/9d267b73553d9f8b/out/plumcraft-text-engine`).
- Issue: #89 (reopened 2026-11-19)
- Cause: confirmed with a scratch project built under both versions (`claude-debug/b27-layout/`): cargo 1.99 builds a test program in `<profile>/deps/`, cargo 1.100 in `<profile>/build/<package>/<hash>/out/`; the workspace's binaries are in `<profile>/` under both. Reproduced on the Mac with `cargo +1.100.0 test -p plumcraft-ffi --test switch`.
- Fix: `plumcraft_editor_core::engines::profile_dir` knows both layouts; `engine_binary` and the two test helpers that build a missing engine (`apps/plumcraft-editor/src/lib.rs`, `crates/ffi/tests/switch.rs`) use it. Test: `a_test_program_finds_engine_binaries_in_either_cargo_layout`. Under 1.100 on the Mac the switch test and the editor's kittests fail before and pass after; on Linux CI the pull request's run had no failing test.
- Fixed: 2026-11-19 on the dev branch `feat/native-n0` only, PR #100 (`b2bd46c`). `main` has its own copy of the lookup in `apps/plumcraft-editor/src/lib.rs` and still fails there; #89 stays open until the dev branch reaches `main`.

### B26. "Wrap seamlessly at the edges" on Simplex Noise's `tileable`

- Reported: 2026-11-11 (found while checking yapocis3 #65 against this port; filed as #84)
- Report: the `tileable` tooltip says "Wrap seamlessly at the edges", but simplex wraps its lattice in skewed coordinates, so the output repeats along the diagonal and its edges don't tile on x or y, on yapocis's backends or here. Measured seams are the same size as with `tileable` off.
- Context: yapocis3 made the same correction to its docs (its `simplex2d` docstring: "The output is not seamless along x or y"); real seamless tiling is yapocis3 #83 (a feature request), which would change this port too. impressionist's copy of the tooltip (its `docs/BUGS.md` B2) gets the same wording in its own branch.
- Issue: #84
- Cause: the tooltip described what tiling was meant to do, not what the kernel does.
- Fix: the tooltip (`crates/array/src/generate/mod.rs`) is now "Wrap the noise lattice; not seamless on x or y (smooth only)". Test: `simplex_tileable_tooltip_does_not_promise_seamless_edges` (`crates/array/tests/catalogue.rs`), which fails on the old text.
- Fixed: 2026-11-12, branch `fix/83-84-simplex-tile`

### B25. Simplex Noise swapped the tile periods on non-square tileable noise

- Reported: 2026-11-11 (found while checking yapocis3 #65 against this port; filed as #83)
- Report: `simplex2d` puts pixel (r, c) at (r / h, c / w) × frequency, but passed `tile_w = w / frequency` as the period of the row-based coordinate and `tile_h = h / frequency` as the column-based one's. yapocis wraps the row-based coordinate with rows / frequency (its kernels' `width` is the rows). Square sizes, and `tileable` off, were unaffected; non-square tileable output differed from yapocis by up to 0.72 (24 × 16, measured). Every `simplex.json` case was square, so none caught it.
- Context: the doc comment also said the numba kernel "is only right for" square sizes, which went stale when yapocis3 #75 (`29f8403`) fixed numba's loop.
- Issue: #83
- Cause: the port took yapocis's `tile_w`/`tile_h` names as columns/rows, where yapocis means rows/columns.
- Fix: `crates/array/src/generate/noise.rs`: the row-based coordinate wraps every `h / frequency` cells and the column-based one every `w / frequency` (`tile_rows`, `tile_cols`; `simplex2d_single`'s parameters renamed `tile_x`, `tile_y`). The doc comment now says what the kernels compute. `crates/array/tests/fixtures/generate/simplex.json` gains two non-square tileable cases (24 × 16 and 12 × 30) recorded from yapocis3 `main` (`6fa5e01`, numba backend) with the plum coordinator's probe `claude-debug/pc8384_simplex_fixture.py`, which first checked that the five existing cases still reproduce exactly; the existing cases are byte-identical. `simplex_matches_numba` reads `height` and `width` when a case has them and checks the shape. It fails on the old port (max error 0.72 at 24 × 16) and passes exactly (0) on the new.
- Fixed: 2026-11-12, branch `fix/83-84-simplex-tile`

### B24. xyY of black is NaN

- Reported: 2026-11-11 (found while porting colormath; filed upstream as yapocis3 #66)
- Report: the colour spaces gave x = y = NaN for black in xyY, and NaN RGB back, because colormath's OpenCL `xyz2xyY` divided 0 by 0, while its numba port gave 0.
- Context: yapocis3 #66 (now closed) settled it: an exact zero sum X+Y+Z has x = y = 0, and Y is kept. The maintainer chose that option "With check" (an explicit `sum != 0` test), and "Patch the 10 values directly" for this repository's fixture.
- Issue: seantrue/yapocis3#66 (closed)
- Cause: the ports copied the kernel's `x / sum`, `y / sum` with no zero check.
- Fix: `if sum != 0 { x/sum, y/sum } else { 0, 0 }`, Y kept, in the three ports: `crates/engine/src/color.rs` (`xyz_to_xyy`), `crates/array/src/colour/mod.rs` (`rgb_to`) and `python/src/plumcraft_engine/color.py` (`_xyz_to_xyy`, with `np.where`). Bit-identical to yapocis3's numbax `xyz2xyY` (jit and numpy fallback) on 100,000 random rows plus black, (0.5, -0.5, 0), -0 and (0.25, 0.25, -0.5). The inverses already gave XYZ 0 at y = 0 and are unchanged. `crates/engine/tests/fixtures/color.json`: the 10 nulls became 0.0, with nothing else changed (a JSON-aware comparison): `spaces.xyY.from_rgb` planes 0 and 1 (x, y) and `spaces.xyY.to_rgb` planes 0, 1 and 2 (r, g, b), each at samples 0 and 41. Tests: the fixture comparisons in `crates/engine/tests/color.rs` and `python/tests/test_color.py` (xyY) fail with the old ports and the patched fixture; the new `xyy_of_black_is_zero` (array) fails with the old port.
- Fixed: 2026-11-11, branch `fix/66-xyY-black`

### B23. A run marks every node queued, not only the ones it computes

- Reported: 2026-11-11 (found by the AI while evaluating run animation; the maintainer chose to fix it first)
- Report: "Go with proposed colors." (approving the run-animation proposal, which listed this as a fix to make first)
- Context: `RunState::request` (`crates/session/src/run.rs`) marked every node of the graph `Queued`, while the engines compute only what the request needs (`docs/scheduler.md` section 1). Clicking a node's title runs that node and what feeds it, yet every node, downstream ones included, showed "queued" until the reply, which then cleared their state, so a node outside the run lost its earlier "done" or "cached". The run's progress counted the whole graph too.
- Issue: none
- Cause: as above; the session never worked out which nodes a request needs.
- Fix: `run::needed` mirrors the scheduler's rule (the targets, or the sinks, plus watched nodes, and their ancestors; a wire into a bound input is dropped). Only those nodes are queued and counted, and the rest keep what they showed. Tests: `run::queued_tests` (four). Not checked in the real editor.
- Fixed: 2026-11-11, PR #81

### B22. Node thumbnails aren't in step with the latest run

- Reported: 2026-11-11 (from an application built on PlumCraft 0.6.1)
- Report: "node thumbnails are not synchronize"
- Context (the maintainer): a node's thumbnail lags behind that node's latest run, and thumbnails from different runs are shown side by side.
- Issue: #70
- Cause: three things in the session's run state (`crates/session/src/run.rs`). (1) A node's thumbnail was replaced only when a run sent a new one: a node done or cached in a run that made no thumbnail (an output with none, a picture that couldn't be read) kept an earlier run's picture under its new result. (2) While a run computed the graph again, each node kept its last thumbnail until it got there, by design so previews don't blink, but nothing marked it as the earlier run's: finished and waiting nodes looked alike. (3) An edit's stale mark went only at the run's reply, so a node this run had already finished stayed stale and its new thumbnail faded like the old ones until every node was done.
- Fix: the reply's thumbnail, or this run's preview, is the node's thumbnail, and none when the run made none. `NodeRun::thumb_generation` says which run a thumbnail came from, and `thumb_is_older` is true while a newer run is computing the node and hasn't sent one. The canvas fades such a thumbnail, excerpt or waveform, with a hover saying it is from the previous run (`canvas::thumb_faded`). A node stops being stale the moment the run made after the edit finishes it. Tests: `run::thumb_tests` (four, two fail without the fix) and `kittest_run_status.rs` (one). Checked in the real editor (`claude-debug/run_status_probe.py`, `claude-debug/b22/4-mid-run.png`): mid-run, the first node is up to date with the new text while the nodes waiting for the slow one keep their earlier excerpts, faded.
- Not fixed here: `stale_since` keeps the first edit that made a node stale. A node already stale when a run is requested and edited again during that run is taken as up to date when the run finishes it. This was already so at the reply, and now it happens when the node finishes.
- Fixed: 2026-11-11, PR #80

### B20. Can't tell which nodes are running

- Reported: 2026-11-11 (from an application built on PlumCraft 0.6.1)
- Report: "can't tell which nodes are actually running"
- Context: the node header shows a spinner while a node runs (`crates/ui/src/canvas.rs`, the run-status drawing).
- Issue: #68
- Cause: a node's stale mark outranked its run state. An edit makes the node and everything downstream stale, and the run that follows (every auto-run) computes them again, but the mark only went at the run's reply. Meanwhile each of them kept the stale border and the word "stale", queued and running alike; running showed only as the 12-point spinner in place of the state dot, and as a progress bar for a node that reports progress. Seen in the real editor (`claude-debug/b21/4-mid-run.png`): the running node looked like the queued ones.
- Fix: while a run computes a node (queued or running) its run state shows, not stale: the border in the queued or running colour (running a little wider), and the header says "queued", or "running" in the running colour with the percentage when the node reports one. `run_view::shows_stale` decides the stale label and border. A run that never reaches the node (cancelled, the engine gone) leaves it stale again. Tests: `kittest_run_status.rs` (two). Checked in the real editor (`claude-debug/run_status_probe.py`, `claude-debug/b20/4-mid-run.png`).
- Fixed: 2026-11-11, PR #79

### B21. A node that has never completed a run shows "stale"

- Reported: 2026-11-11 (from an application built on PlumCraft 0.6.1)
- Report: "stale is not a good status for node that has never completed a run"
- Context: the node header's run status (`crates/ui/src/canvas.rs`, the stale label and border).
- Issue: #69
- Design (the maintainer): a node with no completed run shows **not run**; stale stays for a node whose last result was invalidated by an edit.
- Cause: staleness was tracked for every node with a run record, whatever that run did. A node that failed, was skipped or cancelled, or that a run never reached, went "stale" on the next edit upstream, though it had no result to be out of date.
- Fix: `NodeRun::completed` remembers that a run has completed the node (done or cached), and `NodeRun::is_not_run` is true while none has and none is computing it, for a node never in a run, one left out of a run, or a failed, skipped or cancelled one after an edit. The header says "not run" for those, and the stale label and border are kept for a completed node. A failed, skipped or cancelled node still says so until an edit. `ui.inspect` gives `notRun` per node. Tests: `run::not_run_tests` (session) and `kittest_run_status.rs`. Checked in the real editor (`claude-debug/run_status_probe.py`, pictures in `claude-debug/b21/`): after an edit, the demo's failing node and the one after it say "not run", the rest "stale".
- Fixed: 2026-11-11, PR #78

### B10. Hashing makes content-addressed storage slow for large arrays

- Reported: 2026-11-05
- Report: "Fix the slow hashing issues for content addressed storage."
- Context: `ROADMAP.md` "Next: BLAKE3 handles for large local arrays (D3's measurement)". On an M4 Max with 0.2.1, a 12 MP float32 RGB array (144 MB) crossing between engines through the coordinator adds about 550–600 ms (36 MB: 130–190 ms), more than the nodes on either side; MD5 of 144 MB took 190 ms against 10 ms for BLAKE3 (`b3sum`, multithreaded). The rest of a crossing isn't profiled yet. Code: `Store::publish_array` and its read path (`crates/store`), `crossing_inputs` (`crates/coordinator/src/runner.rs`); the BLAKE3 key helper would go in commonkit (`../pycommonkit`), which D3 held back until a measurement favoured it.
- Issue: #28
- Design (the maintainer, 2026-11-10, of three offered): "Go to BLAKE3 now", then "Yes, publish commonkit".
- Cause: measured on this Mac with a release build (`claude-debug/b10-hash-timing`), 144 MB `<f4`: an array's publish hashed it twice (the engine's handle, then `Store::publish_array` again), about 350 ms of MD5, and a read checked it once more (170 ms): 528 ms, the 550–600 ms reported. BLAKE3 hashes it in 8–12 ms.
- Progress: commonkit 0.1.2 takes 64-digit digests (pycommonkit #17, #18). PlumCraft (`fix/28-blake3-handles`): `b3` handles in proto (shared vectors, checked against Python's `blake3`), the store publishes under the engine's handle without hashing again and checks a handle in its own algorithm, runs ask for `b3` (`RunRequest.algo`) when `hash.b3` was negotiated, and a coordinator clears it when an engine in the run lacks it. 144 MB publish and read back: 528 ms before, 29 ms with `b3` (355 ms with md5 hashed once). The Python SDK followed (`fix/28-blake3-python`): `blake3` as a dependency, `hash.b3` and `data.algos` in its hello, `b3` handles when a run asks, every handle checked in its own algorithm, pyuncommonkit 0.1.2.
- Fixed: 2026-11-10, PRs #74 (Rust) and #75 (Python)

### B18. Port and parameter labels have no translation table

- Reported: 2026-11-06
- Report: "port and parameter labels should have a \"translation\" table, allowing nicer display and formatting. (max_array_width -> Max. width) This table should be both claude AND maintainter editable."
- Context: the node canvas shows a port's name, or the engine's `label` for it, as it comes (`crates/ui/src/canvas.rs`, `port_label`). Raw parameter keys from engines and craft packs (`max_array_width`, `scaleStyles`) show as they are. The table should be a file both the maintainer and Claude can edit.
- Issue: #59
- Design (the maintainer, 2026-11-10, of three offered): "Per pack + user file". Each pack carries its own table, an optional user file overrides it, and core holds only the lookup, no names.
- Fixed: `plumcraft_proto::labels` reads a table (`[labels]`: a port name → label, or a node kind → a table of them; hostile entries skipped, in the hostile-input sweep). Layers, lowest first: the engine's own label; a craft pack's `[labels]` (applied by `plumcraft-craft` to its own nodes) or an application's table (`Session::use_labels`); the user's `labels.toml` beside the preferences. The session keeps the engines' catalogue and relabels it whenever a catalogue or a table changes; `labels.reload` reads the file again and `labels.show` lists what is in effect. PhotoCraft's pack labels `scaleStyles`. Checked in the real editor (`claude-debug/b18_labels_probe.py`): a `labels.toml` written while it ran relabelled the demo's ports on `labels.reload`.
- Not done here: a table for the research software's names (`max_array_width`) belongs to the research software's application, which calls `use_labels` with it.
- Fixed: 2026-11-10, PR #73

### B13. `pdfcraft_runs_a_chain_by_document_id` fails when a save straddles a second

- Reported: 2026-11-05
- Report: "yes, add as B13" (after the test stopped the first `release.sh` run for 0.2.4)
- Context: `crates/craft/tests/pack_pdfcraft.rs:94`, against the installed PdfCraft 0.17.0. The test runs the same chain twice and allows fewer than 16 differing bytes, as PdfCraft's saves depend on the clock in whole seconds through a few bytes of one compressed stream. In `release.sh`'s serial `cargo test` run it failed with "166 of 1498 bytes differ" (`claude-debug/release-0.2.4.log`); alone it passed 3 of 3, and the same commit passed nextest locally and on CI.
- Issue: none
- Seen again: 2026-11-06, in the full `cargo test` gate for #47 (status-bar messages); it passed alone.
- Cause: PdfCraft writes the save time, in whole seconds, as a `D:` date inside a Flate-compressed object stream. A save a second later re-compresses that stream, so hundreds of bytes move and the file can be a byte longer (reproduced with a 1.1 s pause: 487 bytes differ, 1496 against 1497 bytes). The test allowed fewer than 16 differing bytes at equal length.
- Fix: the test compares the two PDFs' streams, inflated, with dates masked and the cross-reference stream left out, and now always pauses past a second so the case it guards is the one it meets. It also checks that the comparison still tells two watermarks apart.
- Fixed: 2026-11-10, PR #71

### B19. A popout doesn't stay on top while the main window is being edited

- Reported: 2026-11-07
- Report: "popout shoud stay on top while main window is being edited"
- Context: reported from a PlumImpress session (PlumImpress runs PlumCraft's editor); popouts are `crates/ui/src/popouts.rs`. Clicking into the main window to edit puts it in front of the popout.
- Issue: none
- Cause: pop-outs were ordinary windows (`WindowLevel::Normal`), so a click into the main window ordered it in front of them. Reproduced in the real editor (`claude-debug/b19_popout_on_top_probe.py`): after raising and clicking the main window, it was listed in front of the pop-out.
- Fix: while one of the editor's windows (the main one or a pop-out) has focus, every pop-out is `AlwaysOnTop`; when another application takes the focus they go back to `Normal`, so they don't float over other programs (`crates/ui/src/popouts.rs`). `ui.inspect` gives each pop-out's `on_top`. The probe: the pop-out stays in front (the floating level, layer 3) after the main window is clicked, drops to layer 0 with Finder in front, and floats again when the editor returns.
- Fixed: 2026-11-10, PR #72

### B17. Packs aren't versioned or chosen by the application's version

- Reported: 2026-11-06
- Report: "Packs should be versioned and selected against the engine (application) version."
- Context: the craft packs (`crates/craft/packs/*.toml`, `plumcraft_craft::manifest`). Just before this, the newly installed craft apps added command parameters (PhotoCraft 0.20.2, SoundCraft 0.17.1, VectorCraft), and the packs were edited to match on branch `craft-baselines`. A pack has no version of its own and doesn't say which application versions it fits, so one pack serves every installed version.
- Issue: #56
- Fixed: 2026-11-06, PR #58

### B16. Not everything tabular can be converted to a TABLE

- Reported: 2026-11-06
- Report: "anything tabular should be convertible to a TABLE (csv, grid objects, etc)"
- Context: Convert (`core.convert`, `docs/research/20-convert.md`) and the TABLE core type. Today the text engine converts Text ↔ Table and Array ↔ Table (6.3), and has CSV readers. Whether a CSV file value, a GridCraft workbook or range, or other tabular craft and pack types have a converter to TABLE is not checked yet.
- Cause: CSV files, CSV text, arrays and GridCraft workbooks (Export CSV → CSV to Table) already reached TABLE. JSON had no converter, so rows such as GridCraft's Read range went JSON → Text → Text to Table and were parsed as CSV. CSV readers knew only commas.
- Issue: #54
- Fixed: 2026-11-06, PR #55

### B15. Not every value can be turned into an array; a binary file can't be cut and reshaped

- Reported: 2026-11-06
- Report: "All values should be reducible to arrays, even if just a bytes array. value -> bytes array -> slice -> shaped array of dtype should be allow truncation of a binary file and casting it to a user determined shape."
- Context: core types and Convert (`core.convert`, `docs/research/20-convert.md`), and the array engine (`crates/array`). The chain asked for is any value → its bytes as a 1-D `uint8` ARRAY → a slice (to cut a binary file short) → a reinterpretation as an array of a chosen dtype and shape.
- Issue: #50
- Fixed: 2026-11-06, PR #53

### B14. A port's value isn't shown on hover unless a wire is connected; clicking a port doesn't watch it

- Reported: 2026-11-06
- Report: "ports should hover reveal the available value even if no wire is connected, click should watch the port value"
- Context: the node canvas (`crates/ui/src/canvas.rs`), seen while working in PlumPipe's window on PlumCraft 0.4.0. Today a node's output is watched by clicking the node's title, which watches its default output (`run.watch {node, port?}`).
- Issue: #49
- Fixed: 2026-11-06, PR #52

### B2. No installer for Intel (x86_64) Macs

- Reported: 2026-11-05
- Report: "No installer for x86 arch on Mac"
- Context: releases carry only `PlumCraft-VERSION-macos-arm64.dmg`, as `packaging/release.sh` builds this machine's architecture. On 2026-11-05 `packaging/macos.sh` gained `CARGO_BUILD_TARGET=x86_64-apple-darwin` (uncommitted) and a test `PlumCraft-0.2.2-macos-x86_64.dmg` was built from the working tree, but it was never run on an Intel Mac and `release.sh` doesn't build or upload it.
- Fixed: 2026-11-05, PR #41. `release.sh` on a Mac builds both DMGs (`arm64` in `target/release`, `x86_64` with `CARGO_BUILD_TARGET`), checks both Rust targets are installed before building, and checksums and uploads both. `macos.sh` checks every binary's architecture with `lipo` and runs the Intel CLI when Rosetta 2 is installed. Both 0.2.3 DMGs built, signed and verified on the M4 Max. **Not yet run on an Intel Mac** (this Mac has no Rosetta 2).
- Issue: #23

### B12. No viewer for STL files

- Reported: 2026-11-05
- Report: "Add viewer for STL files"
- Context: the viewers (`crates/ui/src/viewers/`, registry in `mod.rs`) have nothing for 3D meshes, so an STL value gets the hex dump. The protocol already anticipates meshes: `Media::Mesh` (`crates/proto/src/catalogue.rs`), a `MeshLod` rendition (`crates/proto/src/display.rs:140`), and `model/stl` ↔ `stl` in `crates/proto/src/media.rs:32`. Not yet known: where the STL came from (a file, an engine such as SolidCraft's export) and whether it was ASCII or binary STL.
- Fixed: 2026-11-05, PR #40. A `mesh` viewer (`crates/ui/src/viewers/mesh.rs`) for values typed `model/stl`, binary or ASCII: flat-shaded on the CPU with a z-buffer, turned by a drag, zoomed by scrolling, fitted by a double-click or Fit. The bounded reader is `plumcraft_proto::mesh` and joins the hostile sweep. Both are adapted from SolidCraft (MIT, `THIRD-PARTY.md`) after research found nothing ready to use with egui 0.36 and wgpu. `MeshLod` renditions aren't used yet.
- Issue: #38

### B3. With automatic engines on, the Engine menu still offers every implementation

- Reported: 2026-11-05
- Report: "When \"preferences->engines->select engine automatically\" is enabled, the \"engines->select engine\" should just select the engine, not offer the list of implementations"
- Context: the setting is Settings › Engines › Pick Engines Automatically (`crates/ui/src/settings_view.rs:225`, `engine.auto`); the Engine menu's list of engines and their hostings comes from `engine_choices_menu` (`crates/ui/src/engine_view.rs:423`, called from `crates/ui/src/lib.rs:1509`). Expected: with the setting on, choosing an engine starts it the fastest way it can be hosted (in-process, then a process, then HTTP) without a submenu of implementations.
- Fixed: 2026-11-05, PR #39. With the setting on, `engine_choices_menu` shows each engine as one item running `engine.use` with no mode, which the session already resolves to the fastest hosting (`Engines::best`); with it off, the submenu stays. Tests: `crates/ui/tests/kittest_engine_menu.rs` (the automatic case fails without the fix).
- Issue: #24

### B11. `a_panicking_node_rebuilds_the_engine` raced the engine's rebuild on CI

- Reported: 2026-11-05
- Report: "Add as B11" (after the test failed the first CI run of both #30 and #31)
- Context: `crates/text/tests/conformance.rs`, `a_panicking_node_rebuilds_the_engine`: a node panics and the host rebuilds the engine. It failed the first runs of PR #30 and #31 (runs 42102535342, 42102561899) and both runs of #36 under cargo-nextest (42122511058, 42123346098), and passed on rerun and locally.
- First diagnosis, wrong: too short a 5 s wait.
- Cause: after the panicking run's reply, the test asserted `events.iter().any(EngineDown) || builds == 2`. The `EngineDown` half could never hold: the host pushes it as a session event (no run id), and `collect` polls the run's events only. The other half raced the host, which replies and then rebuilds (`crates/engine/src/host.rs`: `going_down`, `finish`, `rebuild`). Reproduced every time by making the rebuild take 300 ms before it counts; under load on the M4 Max it never failed in 40 runs.
- Fixed: 2026-11-05, branch `fix/33-panic-rebuild-race`. The test reads the session's events and asserts `EngineDown` comes before the run's reply, the order the host guarantees, then waits for the rebuild as before. The slow rebuild stays in the test, so the reply always comes first; 0 of 30 runs failed under load.
- Issue: #33

### B6. npexact has no contraction rule for numpy's runtime-dispatched SIMD loops

- Reported: 2026-11-05
- Report: "In PlumCraft, but remember as bug for future fix in npexact" (on fixing B5)
- Repo: npexact, in `seantrue/yapocis-rs`; not PlumCraft's to fix
- Context: `npexact::random::Contraction::host_default()` (npexact 0.1.1, `src/random/mod.rs:223`) decides fused-or-plain by architecture at compile time: fused on aarch64, plain elsewhere. That fits numpy.random's C, which the wheels compile once, but not numpy's SIMD ufunc loops, which numpy picks at run time and which fuse on x86-64 with FMA3 and AVX2 (B5). PlumCraft borrowed the random rule for Mandelbrot's complex multiply and got x86-64 wrong. B5 is fixed in PlumCraft with its own `numpy_complex_fma()` (`crates/array/src/generate/noise.rs`); npexact should offer that rule (a runtime "numpy's SIMD contraction" beside `Contraction`, with fixtures from both hosts), and PlumCraft should then call it instead.
- Issue: seantrue/yapocis-rs#10
- Fixed in npexact: 2026-11-05, seantrue/yapocis-rs#11 (`5a51589`), with the recorder in seantrue/yapocis3#74. `npexact::complex::Mul` has `array_default()`, decided at run time (fused on arm64, and on x86-64 with FMA3 and AVX2), and `scalar_default()`, decided at build time: numpy scalars are plain on x86-64. npexact 0.1.2 published it (tag `npexact-v0.1.2`).
- Fixed in PlumCraft: 2026-11-05, branch `fix/b6-npexact-complex`. The Mandelbrot kernel calls `npexact::complex::Mul::array_default()` and `mul_c64` (npexact 0.1.2) in place of its own `numpy_complex_fma()`; the golden test passes on the M4 Max and on yellowfin.

### B4. No built-in viewer for XML files

- Reported: 2026-11-05
- Report: "No build in viewer for XML files."
- Context: the built-in viewers are in `crates/ui/src/viewers/` (`basic`, `grid`, `pdf`, `plot`, `wrap`, `yaml`); there is a YAML one but nothing for XML. SVG, which is XML, is drawn as a picture (`crates/ui/src/media.rs`). Read here as "built-in"; no detail yet on where the XML came from (a file node, an engine's value) or what it showed instead.
- Issue: #26
- Fixed: 2026-11-05, branch `fix/26-xml-viewer`. A new `xml` viewer (`crates/ui/src/viewers/xml.rs`) takes XML text (a declaration, or an element after comments) and bytes typed `application/xml`, `text/xml` or `…+xml` (SVG keeps its own), ahead of Text. It shows an outline of elements, attributes, text and comments, with the source a checkbox away and a parse error's position. roxmltree parses it (already in the build through usvg), with DTDs refused, a node limit, and the outline bounded in depth, nodes and bytes; tests sweep truncations, flipped bytes and an entity bomb. Checked in the real editor with `claude-debug/xml_viewer_probe.py` (XML into the demo's `story` node, popped out with `viewer: "xml"`).

### B1. A popout covers the whole screen when the editor is full screen

- Reported: 2026-11-05
- Report: "popout when editor is full screen covers up whole screen"
- Context: the editor (`target/debug/plumcraft-editor`, working tree with the theme and Settings changes) on macOS in full screen; popouts are `crates/ui/src/popouts.rs`
- Issue: #22
- Cause: not a full-screen window of its own. With the editor full screen, macOS's automatic window tabbing made the new pop-out a tab of the editor's window, so it filled the screen and hid the editor. Reproduced in the real editor with `claude-debug/popout_fullscreen_probe.py` (full screen through the accessibility attribute, then `view.popout`): the tab bar showed "Demo — PlumCraft" and "shout.text — PlumCraft".
- Fixed: 2026-11-05, branch `fix/22-popout-fullscreen`. `no_window_tabbing()` (`crates/ui/src/lib.rs`, from `setup_context`) turns automatic tabbing off with objc2-app-kit's safe `NSWindow::setAllowsAutomaticWindowTabbing`; the probe then shows the pop-out as a 760×592 window of its own over the full-screen editor.

### B5. The Mandelbrot kernel doesn't fuse on x86-64, though numpy does

- Reported: 2026-11-05
- Report: "fix test data" (on the first Linux CI run failing `mandelbrot_matches_numba`)
- Context: CI run https://github.com/seantrue/plumcraft/actions/runs/42081191424 (PR #25, ubuntu-latest, x86_64): `crates/array/tests/generate.rs:85`, "mandelbrot 33×21: 9 of 693 pixels differ, max error 9.5e-1"; the 40×30 case matched. Cargo stopped at this test binary, so the rest of the Rust tests and the Python tests didn't run.
- First diagnosis, wrong: that the golden was arm64-only and x86-64 needed its own. On the maintainer's "Record mandelbrot test data on yellowfin", the golden recorded on yellowfin (x86-64, same numpy 2.5.3 and numba 0.68.0, identical numba kernel) matched the committed arm64 one in every pixel of all three cases (`claude-debug/mandelbrot_golden_host.py`).
- Cause: numpy's complex64 multiply fuses its multiply-adds on any CPU numpy dispatches its AVX2/FMA3 loop to, not just on arm64. `claude-debug/complex64_fma_probe.py` matched a fused model on 100% of 200,000 products on both machines and an unfused one on 71%. The kernel (`crates/array/src/generate/noise.rs:163`) picks fusion with `npexact::random::Contraction::host_default()`, a compile-time rule (fused on aarch64, plain elsewhere) that fits numpy.random's compiled C, not the runtime-dispatched SIMD loop. So on x86-64 it runs plain while numpy fuses. The test data is right; the kernel is wrong on x86-64.
- Fix: fuse when numpy's loop would: aarch64, or x86-64 with FMA3 and AVX2 at run time. The rule may belong in npexact, beside `Contraction`. yellowfin and the CI runners both have FMA3 and AVX2.
- Issue: #27
- Fixed: 2026-11-05, PR #25 (`ba68b25`); #27 closed

### B7. The editor's audio tests time out on Linux CI

- Reported: 2026-11-05
- Report: found by the first Linux CI run with `--no-fail-fast` (run https://github.com/seantrue/plumcraft/actions/runs/42083352370); the maintainer asked to record and fix it ("Go for 2,3-5")
- Context: `apps/plumcraft-editor/src/lib.rs` `tests::audio_kittest`: `play_without_an_output_says_so_and_does_not_panic`, `a_1khz_tone_peaks_in_its_spectrogram_bin` and `the_audio_demo_runs_with_waveforms_and_an_audio_view` each panic at `lib.rs:2084` ("timed out waiting for the watched audio", "… the tone's spectrogram", "… the watched audio and its spectrogram") after 30 s. ALSA on the runner finds no sound card. The first test is meant to pass with no output device.
- Cause, found on yellowfin: not the missing sound card (`ALSA_CARD=NoSuchCard` passes, as does 4 CPUs) but the runner's disk. commonkit's store declines to publish when the disk has under `MIN_FREE_GB` (8 GB) free, and after the workspace's build the runner has less, so the watched audio never arrives. With a fresh store root and `COMMONKIT_STORE_MIN_FREE_GB=100000`, yellowfin times out the same three tests the same way. Same cause as B9.
- Fix: CI sets `COMMONKIT_STORE_MIN_FREE_GB=2` for the job. The store behaves as designed; that the editor waits rather than saying the value is unavailable may be worth a look.
- Issue: none
- Fixed: 2026-11-05, PR #25 (`ba68b25`), the CI floor

### B8. An SVG label in `sans-serif` draws nothing on Linux

- Reported: 2026-11-05
- Report: as B7 ("Go for 2,3-5")
- Context: `crates/svg/src/lib.rs:169` `text_is_drawn_with_the_systems_fonts`: "0 inked pixels" on ubuntu-latest, although `fonts()` found fonts (the test returns early when there are none). The label is `<text font-family="sans-serif">Plum</text>`; on the Mac it draws.
- Cause, likely: fontdb takes the generic families from fontconfig on Linux (on yellowfin `sans-serif` is FreeSans, installed, so it passes there), and nothing checks that the family it names is installed. Where it isn't, the label matches no face and isn't drawn. Not reproduced off the runner; the next CI run's failure message names the family if this is wrong.
- Fix: `installed_generics` in `crates/svg/src/lib.rs` points each generic family that names a missing family at an installed one, with a test that sets them all to a missing family.
- Issue: none
- Fixed: 2026-11-05, PR #25 (`ba68b25`); the next Linux CI run passed it

### B9. Large JSON crosses the Python binding as `opaque`, not `blob`, on Linux

- Reported: 2026-11-05
- Report: as B7 ("Go for 2,3-5")
- Context: `python/tests/test_audio.py:69` `test_large_json_crosses_as_a_blob`: `assert ('opaque' == 'blob')` at line 77 on ubuntu-latest; 217 of 218 Python tests pass there, and all pass on the Mac.
- Cause: the same as B7. The store declines the large JSON because the runner's disk is under commonkit's 8 GB floor, so `Plane` reports it `opaque` (`python/src/plumcraft_engine/data.py:239`). Reproduced on yellowfin with a fresh store root and `COMMONKIT_STORE_MIN_FREE_GB=100000`; without the raised floor it passes.
- Fix: as B7.
- Issue: none
- Fixed: 2026-11-05, PR #25 (`ba68b25`), the CI floor
