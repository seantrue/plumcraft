# Bugs

Bugs reported in passing (`bug: …`), kept until fixed. Numbers are never reused. Entries with an issue are also tracked on GitHub.

## Open

## Fixed

### B22. Node thumbnails aren't in step with the latest run

- Reported: 2026-11-11 (from an application built on PlumCraft 0.6.1)
- Report: "node thumbnails are not synchronize"
- Context (the maintainer): a node's thumbnail lags behind that node's latest run, and thumbnails from different runs are shown side by side.
- Issue: #70
- Cause: three things in the session's run state (`crates/session/src/run.rs`). (1) A node's thumbnail was replaced only when a run sent a new one: a node done or cached in a run that made no thumbnail (an output with none, a picture that couldn't be read) kept an earlier run's picture under its new result. (2) While a run computed the graph again, each node kept its last thumbnail until it got there, by design so previews don't blink, but nothing marked it as the earlier run's: finished and waiting nodes looked alike. (3) An edit's stale mark went only at the run's reply, so a node this run had already finished stayed stale and its new thumbnail faded like the old ones until every node was done.
- Fix: the reply's thumbnail, or this run's preview, is the node's thumbnail, and none when the run made none. `NodeRun::thumb_generation` says which run a thumbnail came from, and `thumb_is_older` is true while a newer run is computing the node and hasn't sent one. The canvas fades such a thumbnail, excerpt or waveform, with a hover saying it is from the previous run (`canvas::thumb_faded`). A node stops being stale the moment the run made after the edit finishes it. Tests: `run::thumb_tests` (four, two fail without the fix) and `kittest_run_status.rs` (one). Checked in the real editor (`claude-debug/run_status_probe.py`, `claude-debug/b22/4-mid-run.png`): mid-run, the first node is up to date with the new text while the nodes waiting for the slow one keep their earlier excerpts, faded.
- Not fixed here: `stale_since` keeps the first edit that made a node stale. A node already stale when a run is requested and edited again during that run is taken as up to date when the run finishes it. This was already so at the reply, and now it happens when the node finishes.
- Fixed: 2026-11-11, PR #PRNUM

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
