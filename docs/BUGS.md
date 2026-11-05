# Bugs

Bugs reported in passing (`bug: …`), kept until fixed. Numbers are never reused. Entries with an issue are also tracked on GitHub.

## Open

### B2. No installer for Intel (x86_64) Macs

- Reported: 2026-11-05
- Report: "No installer for x86 arch on Mac"
- Context: releases carry only `PlumCraft-VERSION-macos-arm64.dmg`, as `packaging/release.sh` builds this machine's architecture. On 2026-11-05 `packaging/macos.sh` gained `CARGO_BUILD_TARGET=x86_64-apple-darwin` (uncommitted) and a test `PlumCraft-0.2.2-macos-x86_64.dmg` was built from the working tree, but it was never run on an Intel Mac and `release.sh` doesn't build or upload it.
- Issue: #23

### B3. With automatic engines on, the Engine menu still offers every implementation

- Reported: 2026-11-05
- Report: "When \"preferences->engines->select engine automatically\" is enabled, the \"engines->select engine\" should just select the engine, not offer the list of implementations"
- Context: the setting is Settings › Engines › Pick Engines Automatically (`crates/ui/src/settings_view.rs:225`, `engine.auto`); the Engine menu's list of engines and their hostings comes from `engine_choices_menu` (`crates/ui/src/engine_view.rs:423`, called from `crates/ui/src/lib.rs:1509`). Expected: with the setting on, choosing an engine starts it the fastest way it can be hosted (in-process, then a process, then HTTP) without a submenu of implementations.
- Issue: #24

### B10. Hashing makes content-addressed storage slow for large arrays

- Reported: 2026-11-05
- Report: "Fix the slow hashing issues for content addressed storage."
- Context: `ROADMAP.md` "Next: BLAKE3 handles for large local arrays (D3's measurement)". On an M4 Max with 0.2.1, a 12 MP float32 RGB array (144 MB) crossing between engines through the coordinator adds about 550–600 ms (36 MB: 130–190 ms), more than the nodes on either side; MD5 of 144 MB took 190 ms against 10 ms for BLAKE3 (`b3sum`, multithreaded). The rest of a crossing isn't profiled yet. Code: `Store::publish_array` and its read path (`crates/store`), `crossing_inputs` (`crates/coordinator/src/runner.rs`); the BLAKE3 key helper would go in commonkit (`../pycommonkit`), which D3 held back until a measurement favoured it.
- Issue: #28

## Fixed

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
