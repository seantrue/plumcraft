# N1 slice 6: values become pictures off the UI thread

Branch `native/n1-media`, working tree only: nothing committed, staged or stashed. 2026-11-19.

## Summary

- **New crate `plumcraft-media` (`crates/media`).** It holds the toolkit-free decoders and the **media service**. A request by value reference answers at once with `Ready`, `Pending`, `Missing` or `Failed`. The engine's `resolve` and the decoding run on worker threads, with a bounded queue, cancellation, a byte-budgeted LRU cache and a wake. It pulls no GUI toolkit; `scripts/check-core-toolkit-free.sh` now checks it beside the core.
- **Rule 6 holds for values in the egui editor.** Thumbnails, waveforms, the Watched panel, compare, pop-outs, wire previews and application value editors all read through the service. None calls `resolve` on the UI thread.
  - Measured against an engine whose `resolve` takes 300 ms: the longest frame went from 317–640 ms to 4–35 ms.
  - Resolves on the editor's thread went from 13 of 13 to 0 of 11.
- **A real "not yet" state.** `ThumbMiss::Pending` is drawn as the bare placeholder (or the waveform's ground), labelled as the preview it becomes. `Watched.pending` is "reading…", where it used to be taken for "gone".
- **C ABI.**
  - `plum_blob` never waits for an engine. It lends what is already here (lent bytes, the local store, finished results) and otherwise answers the new `PLUM_PENDING` (1).
  - New requests: `media.get {handle, want, px?}` and `media.stats`. `plum_poll` gains `media`.
  - New config key `media: {cache_mb, workers, queue}`.
  - The C function list is unchanged. New tests cover an ARRAY thumbnail and an IMAGE picture, pending then ready, against a 300 ms engine.
- **Pixels are byte-identical.**
  - The same probe gives the same 202 outputs before and after (hashes compared).
  - The service's own path reproduces 183 of them byte for byte.
- **Tests.**
  - 1,342 → 1,367: 25 new, none gone.
  - The same single expected failure in three b3 runs and one md5 run.
  - Python: 224 passed under md5 and under b3.
  - fmt, clippy, the hash gates and the toolkit gate are all clean.
- **Edits to existing tests:**
  - one line in `kittest_viewer_tools.rs`, so a test waits for the picture it asserts on;
  - additions to `engine_thread.rs`.
- **Preferences.** The maintainer's `prefs.json` and `trusted.json` are byte-identical before and after.
- **PlumImpress** checks and passes its editor, recipe and view tests with no source change, thanks to a read-ahead (below).
- **Memory.** Peak memory rose about 10% on a 500-node graph, whatever the cache budget (numbers below).

## The cut

| What | Where now | Was |
|---|---|---|
| `Rgba`, `shrink` | `plumcraft_media::raster` (the core re-exports it at `plumcraft_editor_core::raster`) | core `raster.rs`, ui `media.rs` |
| Picture files: `png_size`, `picture_size`, `decode_picture`, `decode_png`, `decode_numbers`, `image_channel_names`, limits | `plumcraft_media::decode` (feature `images` for non-PNG) | ui `media.rs` |
| An engine's rgba8 rendition to a picture (`rendition`), `MAX_THUMB_SIDE` | `plumcraft_media::decode` | ui `run_view.rs` `rgba_image` |
| 1-d arrays as numbers (`numbers`) | `plumcraft_media::decode` | ui `watched.rs` |
| SVG: `parse_svg`, `render_size`, `render_svg`, `rasterise`, limits | `plumcraft_media::svg` (feature `svg`) | ui `media.rs` |
| Audio: `Peaks`, `Clip`, `decode_inline`, `resample_linear`, `Spectro` (with `ramp_positions` and `rgba`), `spectrogram`, constants | `plumcraft_media::audio` (feature `spectrogram` for the FFT) | ui `audio.rs` |
| The media service (requests, workers, queue, cache, wake) | `plumcraft_media::service` | new; replaced `Fulls` and the resolves in `Thumbs` |
| Tying a document's engine link to the service (`Follow`); the session's `docs.resolver` through it; a non-blocking adapter for value editors; `read_ahead` | `plumcraft_editor_core::values` | new |
| Uploading pictures to egui textures (`Thumbs` texture caches, `load_picture`, `RgbaImage`, `SpectroImage`); every viewer's drawing, zoom and pan; `ImageView`, `SvgView` and `AudioView` state; compare's layout | stays in `crates/ui` | |
| `probe`, `diff`, `dtype` | stay in the core (see "What remains") | |

- **Dependencies: media ← core ← ui and ffi.** The media crate depends on neither the host nor the core. A resolver is a plain `Arc<dyn Fn(&DataRef) -> Option<Blob>>`, which the core builds from an engine `Connection` (`values::resolver`). So there is no cycle.
- **Direct dependencies of `plumcraft-media`** (`cargo tree`, `claude-debug/native-n1/s6-cargo-tree-media.txt`): `base64`, `image`, `plumcraft-proto`, `plumcraft-store`, `plumcraft-svg`, `png`, `rustfft`, `serde_json`.
- **Its full tree,** all features and every target, is 166 crates (`s6-cargo-tree-media-all.txt`). None is egui, eframe, winit, wgpu, rfd or `plumcraft-ui`. The gate prints "core gate: ok (plumcraft-editor-core and plumcraft-media build no GUI toolkit)".
- `--no-default-features` builds, passes clippy and passes its tests.

**Where the brief differs from the code: arrays become pictures in the engine, not the UI.**

- Colour maps, ranges, planes and named pictures are made by the engine SDK (`plumcraft_engine::arrays::views`, `rgba8`, `thumb`). They cross as `rgba8` renditions.
- `array_view.rs` only shows them with their stats. `slice_view.rs` is wire slicing, not array slicing.
- So for arrays the media crate decodes renditions; there was no mapping to move.

## The media service

- **Requests.** `MediaService::get(&DataRef, Want) -> Fetch<Made>`, with typed helpers (`bytes`, `rendition`, `peaks`). `Want` is one of:
  - `Bytes`;
  - `Rendition` (rgba8 to a picture);
  - `Peaks`;
  - `Picture(px)` (a file decoded and shrunk);
  - `Svg(px)`;
  - `Numbers` (a picture file's own numbers).

  The key is (handle, or the locators' text, and `Want`). A `Source` trait lets code take either the service or `Blocking(resolve)`; the old `Fn(&DataRef) -> Option<Blob>` signatures are kept as wrappers.
- **States.**
  - `Ready(Made)`.
  - `Pending`: queued or being made; ask again after a wake.
  - `Missing`: the engine has no bytes for it, or there is no engine.
  - `Failed(reason)`: read but not what it claims (a hostile shape, a picture that won't decode, or a resolver that panicked, caught).
- **Threads.** `Config::workers` threads (default 2), named `plumcraft-media-N`, started on first use.
  - Waveform peaks inline in their reference (≤ 64 KiB base64) are made in place, as the canvas always decoded them; no engine is involved.
  - Everything else goes through the connection on a worker. That includes inline bytes: a connection may refuse a value, as `kittest_viewer_tools`' remote-engine stand-in does.
  - If a thread can't be spawned, the request is made in place rather than never.
- **Queue.** Bounded (default 512). A request past the bound answers `Pending` without being queued, and is queued when asked again.
- **Cancellation.**
  - A shell calls `frame()` once a frame. A queued request unasked for `keep_frames` frames (default 120) is dropped before its work starts: a node scrolled away, a superseded run's value.
  - `set_resolver` (a new engine link) cancels everything queued for the old link. So does `cancel(ref)`.
  - Work already inside an engine call can't be stopped; its result is kept only if its link is still current.
- **Cache.**
  - A byte budget (default 256 MiB; 64 MiB in the egui editor, which keeps textures besides; `PLUMCRAFT_MEDIA_CACHE_MB` overrides it; the C ABI's `media.cache_mb`).
  - Eviction is least recently asked for first. A result not yet collected is never evicted until it has gone unasked for `keep_frames` frames, so a value larger than the budget is still handed over once.
  - At most 16,384 entries, misses included.
  - Bytes are also indexed weakly, so whatever still holds them (the panel, a pop-out, a C lease) shares one copy. That is what `Fulls` did.
  - Misses are kept until `forget_misses()`, which the egui shell calls on each run reply, as `Thumbs` did.
  - Content-handled results survive engine switches; handle-less ones (path locators) don't.
  - `drop_caches()` and `set_budget()` are there for a memory warning.
- **Waking.** `set_wake(Arc<dyn Fn()>)` is called on a worker when a result lands, with no payload. In egui it is `request_repaint`; in the C ABI it sends `Msg::Media` to the core thread, which puts the finished items into `plum_poll`'s `media` and wakes the shell.
- **Stopping.** Dropping the last clone stops the workers after their current job. Nothing is joined. A test drops a service whose worker is inside a 3 s resolve in well under 100 ms.
- **Hostile input.**
  - `crates/media/tests/hostile.rs`: about 500 hostile blobs, 12 hostile files, and every `Want` over about 1,600 hostile references through the service. No panic, no hang, no allocation of a claimed size.
  - It found a real overflow panic in `shrink` (`w * h * 4` with huge sizes, pre-existing). It is fixed with checked arithmetic; valid pictures take the identical path.

## Every formerly UI-thread `resolve`

| Where it was (slice 3's list) | Where it runs now |
|---|---|
| `run_view.rs` `Thumbs::get` (node thumbnails, wire-preview source thumbnail, Pane renditions and sheet) | Worker (`Want::Rendition`); `Thumbs::get_from` uploads the texture when Ready |
| `run_view.rs` `Thumbs::picture` (array view pictures, compare sides, pop-outs) | Worker (`Want::Rendition`) |
| `run_view.rs` `Thumbs::peaks` via `audio::peaks_of` | Inline peaks in place, without the engine (as before); otherwise on a worker |
| `watched.rs` `Fulls::read` via `full_reader`: the Watched panel (`watched.update`), `panel_pane.sync`, `sync_media`, `sync_compare`, `popouts.rs`, `wire_view.rs` | Worker (`Want::Bytes`), through `Watched::update_from`, `Pane::sync`, `CompareView::sync` |
| `lib.rs` `value_editor_windows` (application value editors' `cx.resolve`) | Never waits: `values::nonblocking` gives the bytes once here, `None` meanwhile. `values::read_ahead` asks for every reference in the run's editor payloads as reports arrive, so an editor usually finds its picture read |
| `session.docs.resolver` (`doc.open`, `value.open`), set in `lib.rs` `with_link` and `drain_engine` | **Remains in place**, now through `values::docs_resolver` (`MediaService::blocking`: shares what was read; reads in place if not). These commands reply with the copy they made, so moving them changes their replies. `doc.open` refuses remote engines anyway, so only `value.open` on an HTTP engine still fetches on the UI thread. For the asks/`Document` slice |
| `switchboard.rs` `relinked`: `session.docs.resolver` straight from the connection | Remains: the egui shell overwrites it every frame with the service's. The C ABI core still uses it for `doc.open`/`value.open` on its core thread (not the shell's thread) |
| FFI `Shared::blob` (`plum_blob`) | Never waits for an engine. It lends bytes already lent, then the local store by handle, then the store by a known reference's locators (a mapping or a read on this machine: no engine, no network; a deflated blob is still inflated in place, about 12 ms for 64 MB). Anything else goes to the service: `PLUM_PENDING`, then the feed's `media` |
| `Session::connection()` used by application commands (PlumImpress `cmds.rs` `step` reads `s.docs.resolver`) | Remains (application commands are synchronous) |

## The thread-rule test and frame stalls

`crates/ui/tests/engine_thread.rs` gains a scenario.

- **The engine.** An in-process engine whose values are:
  - two 192 × 256 × 3 float arrays;
  - a 192 × 256 float array;
  - a 640 × 480 PNG;
  - a stereo tone.

  It is wrapped in `SlowValues`: every `resolve` takes 300 ms and is counted when made on the editor's thread.
- **The scenario.** Run (the canvas's thumbnails and waveform); watch each value (the Watched panel reads it in full); open a pop-out; pin A and B and compare by difference.
- **`values_never_wait_for_an_engine` (runs in CI) asserts:**
  - every phase's value arrives;
  - no frame reaches 150 ms;
  - zero `resolve`s on the editor's thread;
  - at least one `resolve` was made.
- **`value_stalls` (ignored)** prints each phase at 0 and 300 ms.
- **On the baseline it fails.** For the before numbers I restored HEAD's versions of only the files I had changed (backed up in `claude-debug/native-n1/s6-wip-backup/`), ran the new test there, and restored mine (each `cmp`-checked).

Debug build, this M4 Max. Longest frame per phase, `resolve` taking 300 ms (raw: `claude-debug/native-n1/s6-stalls-before.txt`, `s6-rule-before.txt`, and the after output in this report):

| Phase | Before | After |
|---|---|---|
| Run, then the canvas's thumbnails and waveform | 624.1 ms | 7.3 ms |
| Watch n1 (3-plane array) | 640.2 ms | 15.7 ms |
| Watch n2 (3-plane array) | 627.5 ms | 4.4 ms |
| Watch n3 (1-plane array) | 623.7 ms | 4.4 ms |
| Watch n4 (PNG) | 321.7 ms | 8.9 ms |
| Watch n5 (tone) | 349.1 ms | 35.3 ms |
| A pop-out | 321.3 ms | 13.0 ms |
| Compare A and B | 318.8 ms | 28.2 ms |
| `resolve`s on the editor's thread | 13 of 13 | 0 of 11 |

- **With no delay** the phases are alike before and after (8 to 33 ms), except the tone: 97 ms before, 88 ms after.
- **The tone's 35 to 97 ms** is the spectrogram, which `AudioView` still computes in place for clips under `SPEC_SYNC_SAMPLES`. It is not an engine wait. I shortened the test's tone so that phase measures what the test is about.

## Pixel identity

- **The probe.** `claude-debug/native-n1/s6-pixels/` is a scratch crate built into the workspace `target/`. It decodes, through `plumcraft_ui`'s public paths:
  - float, u8 and i16 arrays of 1 to 4 planes: the engine's thumbnail and every view mode, decoded as the editor decodes them;
  - a PNG, a JPEG and a 16-bit PNG: full, shrunk to 64, and their numbers;
  - an SVG at three sizes;
  - stereo peaks from a clip and from a peaks blob, and their columns;
  - a spectrogram's dB and its pictures in Paper and Dusk;
  - compare differences (mean and per channel) of float and wide u8 values.
- **Result.** It ran before any change (`s6-pixels-before/`, 202 files) and after (`s6-pixels-after/`). `s6-pixels-before.sha256` and `s6-pixels-after.sha256` are identical.
- **One line added to the probe after:** `use plumcraft_ui::audio::SpectroImage`, since `Spectro::image` is now an extension trait.
- **The service's own path.** `s6-pixels/src/bin/service.rs` makes the arrays, images and SVGs through `MediaService` (what the editor now reads through). It matches 183 of the before files byte for byte, 0 differing (`s6-pixels-service.txt`). The other 19 (peaks, spectrograms, compare) go through moved but unchanged functions, covered by the 202-file comparison.
- **The repository's own tests** (kittest picture checks, decoding tests) all pass.

## Memory

- **What was measured.** `value_memory` (ignored, in `engine_thread.rs`): 500 nodes, each a distinct 96 × 128 × 3 float array, run, then about 4 s of frames with every thumbnail on the canvas. It ran under `/usr/bin/time -l`, debug build.
- **Raw output:** `claude-debug/native-n1/s6-memory-before.txt`, `s6-memory-after.txt`.

| Run | Max RSS | Peak footprint |
|---|---|---|
| Before | 690 MB, 673 MB | 610 MB, 603 MB |
| After, 64 MiB cache (the editor's default) | 764 MB, 754 MB | 668 MB, 670 MB |
| After, 256 MiB cache | 755 MB | 671 MB |
| After, 16 MiB cache | 747 MB | 667 MB |

- **The rise is about 60 to 90 MB (~10%), and the cache's budget doesn't move it,** so it isn't what the cache holds.
- My guess, not measured: each thumbnail is now copied out of the mapped store segment into an owned buffer on a worker, before egui's image copy; and the memory is allocated on one thread and freed on another. Handing renditions to the shell as the mapped `Blob` would remove the copy.
- **Budget and eviction:**
  - default 256 MiB; the egui editor 64 MiB; configurable by `media.cache_mb` in the core's config (a phone: 32 or 64);
  - least recently asked for first;
  - uncollected results protected for `keep_frames`;
  - `drop_caches()` for a memory warning (not yet wired to `lifecycle.memory`, which doesn't exist).

## The C ABI

- **`plum_blob(core, key, len)`.** The key is a store handle, or a derived key from `media.get`:
  - `derive:rgba:<h>`, `derive:image:<h>@<px>`, `derive:svg:<h>@<px>`, `derive:peaks:<h>`, `derive:numbers:<h>`.
  - Status: `PLUM_OK` with a lease (a store `Blob`, an RGBA8 picture with straight alpha, or f32 peaks); `PLUM_PENDING` (1, new); `PLUM_ERR_MISSING`; `PLUM_ERR_UNREADABLE` (-5, new); `PLUM_ERR_ARG` for a malformed `derive:` key.
  - Leases are now a `Lease` enum; `plum_release` and `plum_leases` are unchanged.
- **`media.get {handle, want?, px?}`** replies at once with `{key, handle, want, status, w?, h?, len?, format?, dtype?, shape?, channels?, bins?, reason?}`.
  - `want` is one of `bytes` (the default), `rgba`, `peaks`, `image`, `svg`, `numbers`; `px` is 1 to 4096 (default 256).
  - The handle may be one the engine sent in events or any store handle.
- **`media.stats`** gives the service's counters.
- **`spike.slowResolve {ms}`** adds a delay to every read through the engine, for tests and measurements.
- **`plum_poll`** gains `"media": [...]` (the same object as `media.get`'s reply) when requests finish.
- **Config:** `media: {cache_mb, workers, queue}`; a bad value fails `plum_open` with a reason naming `media`.
- **Threads:** the core thread never waits. Workers resolve through the current connection (following switches), else the store by handle.
- **Header:** `PLUM_PENDING` and `PLUM_ERR_UNREADABLE` are defined, and the blob and poll comments are updated. The function list and `PLUM_ABI_VERSION` are unchanged; `header.rs` passes.
- **Tests (`crates/ffi/tests/media.rs`, new).** `a_thumbnail_and_a_picture_are_pending_then_lent_and_nothing_waits_for_the_engine` runs on the array engine in-process: constant → frame → Encode Image, run, then `spike.slowResolve 300`. For the frame's rgba8 thumbnail (an ARRAY value) and for the PNG (an IMAGE value, at `px` 128):
  1. `media.get` replies `pending` in under 150 ms.
  2. `plum_blob` of the key is `PLUM_PENDING` in under 50 ms.
  3. The feed's `media` reports `ready` after at least 250 ms, with `w` and `h`.
  4. `plum_blob` then lends `w·h·4` bytes in under 50 ms.
  5. Those bytes equal the rendition's own bytes (lent from the store) and `decode_picture` of the PNG's bytes (128 × 85; centre pixel 63, from 0.25).
  6. Asked again: `ready` from the cache.

  The same test also checks:
  - hostile `media.get` params and `derive:` keys are refused;
  - a PNG asked for as `rgba` fails with a reason and is `PLUM_ERR_UNREADABLE`;
  - every lease is released.

  `a_bad_media_config_is_refused_and_the_header_names_the_new_statuses` covers the config and the header. The existing `c_abi.rs` passes unchanged: the 64 MB repeat is lent at once from the store.

## Tests

| Run | Ran | Passed | Failed | Skipped | Failing |
|---|---|---|---|---|---|
| Baseline b3 (`s6-base-b3`) | 1,342 | 1,341 | 1 | 10 | `plumcraft-craft::params_coverage hidden_params_match_the_baseline` |
| Final b3, run 1 (`s6-final1-b3`) | 1,367 | 1,366 | 1 | 12 | the same |
| Final b3, run 2 (`s6-final2-b3`) | 1,367 | 1,366 | 1 | 12 | the same |
| Final b3, run 3 (`s6-final3-b3`) | 1,367 | 1,366 | 1 | 12 | the same |
| Final md5 (`s6-final-md5`) | 1,367 | 1,366 | 1 | 12 | the same |

- **Names** (`s6-base-names.txt` against `s6-final1-names.txt`; runs 1 and 2 identical): 25 new, none gone.
  - `plumcraft-media`: 17 unit tests and 3 hostile.
  - `plumcraft-editor-core values::tests`: 2.
  - `plumcraft-ffi::media`: 2.
  - `plumcraft-ui::engine_thread values_never_wait_for_an_engine`.
  - The two new skips are the ignored `value_stalls` and `value_memory`.
- **No flake, and no leak:** no `LEAK` in any nextest log, and no `shm_foreign_digest` or `shm_bad_key` mark in any run.
- **Repeats while iterating:** the FFI suite three times, and the viewer kittests three times, all green.
- **Python, as CI runs it:** 224 passed under md5 and under b3; `ruff check` clean.
- **Gates:**
  - `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`: clean.
  - `scripts/check-hash-gates.sh`: ok.
  - `scripts/check-core-toolkit-free.sh`: ok for both crates.

## Existing test files touched

1. **`crates/ui/tests/kittest_viewer_tools.rs`, `scrolling_in_past_the_picture_asks_for_detail_once`: one line.** The `until` that waits for the larger picture now also waits for `!h.state().media_busy()`.
   - Why it was forced: the 2000 × 1500 picture is read and decoded on a worker now. The test asserted on it three frames after its run had finished, which it can no longer count on.
   - What a person sees: the larger picture a frame or two later, while the smaller one is still shown. Nothing else.
   - The assertion is unchanged. This is slice 3's `engine_busy()` pattern.
2. **`crates/ui/tests/engine_thread.rs`: additions only.** `SlowValues`, the values engine, `values_never_wait_for_an_engine`, `value_stalls`, `value_memory`. No existing line was changed or removed.
3. **Source modules with inline tests.** Their `#[cfg(test)]` blocks are untouched. Only imports moved (`#[cfg(test)] use` for `base64` and `Locator` in `audio.rs`), and moved items are re-exported so `use super::*` still finds them.

## Preference checksums

| File | Before my first run | After my last |
|---|---|---|
| `~/Library/Application Support/plumcraft-editor/prefs.json` | `9221fa662e648228030c4af3ca118330e0e6a4a950c1db4206dd66a688aca727` | the same |
| `~/Library/Application Support/plumcraft-editor/trusted.json` | `a10d6fc822e72d77cbf715c89bde4fa809897db6cde6ccdf7be80341464f4704` | the same |

- The run used `PLUMCRAFT_EDITOR_PREFS=claude-debug/native-n1/s6-probe/prefs.json`, and `ui.inspect`'s `prefs.path` names that file.
- `prefs.json` differs from slice 5's recorded hash. That change predates this slice.
- Records: `s6-userprefs-before.txt`, `s6-userprefs-after.txt`.

## The real editor

`claude-debug/native-n1/s6_probe.py` drove `target/debug/plumcraft-editor --engine array,audio --control 0`, with replies in `s6-probe/` and notes in `s6-probe/notes.txt`.

- **The graph:** Constant (300 × 200 × 3 at 0.25), then Frame (margin 20 at 0.9), then Encode Image (PNG), and a Tone. `ui.invoke graph.tidy`, `ui.set {fit}`, `run.watch`, run, `ui.waitRun`.
- **What `ui.inspect` showed, polled every 50 ms throughout** (the slowest reply was 60 ms):
  - the framed array: `full: true`, one picture;
  - the PNG: `state: ready`, size 300 × 200, texture 300 × 200;
  - the tone: clip 1 channel × 48,000 frames at 48 kHz, spectrogram ready, peak at 421.9 Hz (a 440 Hz tone; the bins are 46.9 Hz);
  - the pop-out `n2.framed`: `full: true`, viewer `array.pictures`;
  - compare A = framed, B = constant, `|A−B| max 0.75 · mean 0.201 · 55200 of 180000 elements differ`.
- **Then** side-by-side, tidy and fit again, and `ui.screenshot`: `s6-probe/s6-editor.png`, 2880 × 1720, from the window.
- **In the picture:**
  - the four nodes, tidied, all "cached";
  - the array thumbnails: the dark 0.25 field, with the framed node's light 0.9 margin (checked in a crop, `crop-frame.png`);
  - the Tone's waveform;
  - the Watched panel in compare (A | B), with both pictures and the per-channel stats;
  - the status bar: "Array + Audio 0.7.0 · connected", "last run 198 ms".

  Nothing looked different from before.

## Downstream effects

- **PlumImpress.** I copied it, without `target`, `.git`, `dist` or `claude-debug`, to `claude-debug/native-n1/s6-plumimpress-check/`, with slice 5's `.cargo/config.toml` patch, into the workspace `target/`.
  - `cargo check --tests` exits 0 with no source change.
  - `editors` (7), `open_recipe` (2) and `views` (5) pass, three runs out of three.
  - Without the read-ahead, 3 of its 7 editor tests failed. Its Curves and Mask editors read `cx.resolve` once when opened and showed "gone, run again" until a later frame.
  - With the read-ahead they pass. The repository itself was not touched.
- **A source change PlumImpress would want** (none is needed to build): its editors treat `cx.resolve` returning `None` as "gone".
  - On an HTTP engine the bytes can take longer than the read-ahead. They then show "the step's picture is gone: run again" or "Preview not available" for that moment. They do retry each frame, so it heals.
  - Proposed: a `Context::pending(&DataRef) -> bool` (or a three-state read) in a later slice, and "reading…" in those two editors.
- **API changes a downstream crate would see:**
  - **New crate** `plumcraft-media`, re-exported by the core as `plumcraft_editor_core::media`.
  - **New in the core:** `plumcraft_editor_core::values` (`Follow`, `resolver`, `docs_resolver`, `nonblocking`, `read_ahead`, `MAX_READ_AHEAD`).
  - **New on `EditorApp`:** pub field `media_service`, `media_busy()`, `wait_media(timeout)`, `with_media(config)`, and the free function `plumcraft_ui::media_config()`. The private `fulls` field is gone.
  - **`plumcraft_ui::run_view::ThumbMiss`** gains `Pending`; an exhaustive `match` downstream breaks (PlumImpress has none).
  - **New on `Thumbs`:** `get_from`, `picture_from` and `peaks_from` (taking a `plumcraft_media::Source`). `get`, `picture` and `peaks` keep their signatures and read in place.
  - **Signatures that changed** (none is used by PlumImpress):
    - `Pane::sync`, `CompareView::sync`, and `canvas::RunView.resolve` (now `.source`) take a `&dyn plumcraft_media::Source` in place of the resolve and full closures.
  - **`plumcraft_ui::audio::Spectro::image`** is now the extension trait `SpectroImage`; import it. The audio, `media` and `watched::numbers` items are re-exports at their old paths.
  - **`Watched`** gains a pub `pending` field and `update_from`; `update` is unchanged.
  - **The UI text** shows "N bytes, reading…" under the summary while a value is on its way. It used to say "gone (reaped)" in that moment, and only when a read returned nothing.
  - **The C ABI:** `PLUM_PENDING` and `PLUM_ERR_UNREADABLE`; `media.get`, `media.stats` and `spike.slowResolve`; the `media` config key and poll item. `plum_blob` may now answer `PLUM_PENDING` for a value only an engine can read.
- **The CI workflow:** unchanged. It already calls the gate script.

## What of item 1 remains

| What | Where | Lines | What it needs |
|---|---|---|---|
| `probe`, `diff`, `dtype` into `plumcraft-media` | `crates/editor-core/src/{probe,diff,dtype}.rs` | 461, 287, 75 | `probe` uses the core's `plain::{Point, Rect}`, `diff` uses `plain::Color` and `plumcraft_session::viewer_cmds::DiffMode`. Move `plain` (or give these `[f32; 2]` and `[u8; 4]`) and take `DiffMode` from the session, then re-export from the core |
| Decoding a full image and its numbers, SVG parse and render jobs: one thread per job (`media::Job::spawn`), unbounded, not cancelled | `crates/ui/src/media.rs` `ImageView::sync`, `sync_numbers`, `SvgView::sync`, `rerender` (lines ~400–590 now) | ~120 | `Want::Picture`, `Want::Numbers` and `Want::Svg` exist in the service. These views already hold the bytes, so this is about bounding threads, not about rule 6 |
| Compare's difference job | `crates/ui/src/compare_view.rs` `sync_diff` | ~45 | A `Want::Compare(a, b, mode)` keyed `derive:compare:…`, so a native shell gets the difference picture as a lease |
| The spectrogram computed in place for short clips, and its thread for long ones | `crates/ui/src/audio.rs` `AudioView::sync`, `poll` | ~60 | A `Want::Spectrogram` in the service, and `Spectro::rgba` with the theme's ramp in the core; also removes the 35–97 ms frame |
| The audio view decoding inline peaks each frame for drawing | `audio.rs` `AudioView::ui` | ~10 | `peaks_from` through the service |
| PDF pages (hayro) | `crates/ui/src/viewers/pdf.rs` | 198 | A `pdf` feature and `Want::PdfPage(page, px)`; Apple uses PDFKit (decision 9) |
| Mesh shading | `crates/ui/src/viewers/mesh.rs` | 459 | `Want::Mesh(camera, w, h)` |
| Grid, plot, XML, basic viewers' value reading | `viewers/{grid,plot,xml,basic}.rs` | — | They read `full`, which already comes from the service; only drawing remains |
| `session.docs.resolver` for `doc.open` and `value.open` | session `doc_cmds.rs` / `open_cmds.rs`; FFI through `switchboard.rs:473` | — | Those commands reply after their copy (a `pending` reply and a later outcome, like `doc.open`'s existing `pending: true`); it belongs with the asks/`Document` slice |
| `lifecycle.memory` calling `drop_caches` and `set_budget`; `viewer.size` setting `px` | contract 4.4, 10.4 | — | The lifecycle methods don't exist yet |

## For the last N1 slices

- **The menu model.**
  - Nothing here touches menus.
  - `MediaService::stats()` could feed a status cell ("reading 3 values…") through the same model, if wanted.
- **Startup without waiting for the engine.**
  - The service starts with no resolver, and `Follow` points it at the link whenever one appears.
  - A shell that opens before its engine draws `Pending`/`Missing` placeholders and fills in on the first wake.
  - `with_link` still reads `schema` in place (slice 3's item 3).
- **A core `Document` owning `pump_runs`, `drain_engine` and `pump_saves`.** It should own the `MediaService` and `Follow` too, with:
  - `forget_misses` on each run reply;
  - `read_ahead` on events;
  - `frame()` per frame;
  - the wake.

  These are four lines in the egui shell's `drain_engine` and `pump_runs` today, and the FFI's `Worker` duplicates its own (`Shared.media`). One `Document` would hold one service per document (decision 8). Whether documents should share a cache (content handles are global) is open; a process-wide service, with per-document `Follow`s, would mean a resolver per key instead of one per service.
- **A log.** The service has counters (`stats()`) but logs nothing. A worker's `Failed` reason ("the decoder stopped", a shape error) is today shown in place: the preview's text, the feed's `reason`. Under the log facade these should be `warn` under target `media`.
- **Rule 6, remaining:** `value.open` on an HTTP engine, the in-process spectrogram for short clips, slice 3's startup schema, and slice 3's in-process connect.

## Lines of code

From `git diff --numstat`, plus the new files.

| Area | Added | Removed | Notes |
|---|---|---|---|
| `crates/media` (new) | 2,018 | 0 | `service.rs` 1,059 (about 210 tests), `audio.rs` 340, `decode.rs` 256, `svg.rs` 78, `raster.rs` 66, `lib.rs` 35, `tests/hostile.rs` 155 |
| `crates/editor-core` | 171 | 10 | `values.rs` 160 (about 50 tests), the `raster` re-export, `lib.rs`, `Cargo.toml` |
| `crates/ffi` | 454 | 36 | `core.rs` +242/−23, `lib.rs` +21/−11, header +10/−2, `tests/media.rs` 179 (new) |
| `crates/ui` src | 238 | 550 | `audio.rs` −233 net, `media.rs` −165 net, the rest re-routed through the service |
| `crates/ui` tests | 278 | 1 | `engine_thread.rs` +277, `kittest_viewer_tools.rs` one line |
| Manifests, lock, gate script, guide | 40 | 11 | |
| **Total** | **3,199** | **608** | Net +2,591, of which about 870 lines are tests. `crates/ui/src` is 312 lines smaller |

## What the docs should say

- **`docs/STATUS.md`:** N1 slice 6 is done.
  - `plumcraft-media` exists: the decoders and the media service.
  - Every `resolve` the egui editor's thread made now runs on workers, with a pending state, except `doc.open` and `value.open`.
  - On the C ABI: `plum_blob` never waits, `PLUM_PENDING`, `media.get`.
  - Pixels are byte-identical. Test counts went 1,342 → 1,367, with the same single local failure.
  - Peak memory is about 10% higher on a 500-node graph.
- **`docs/research/24-native-shells.md`:**
  - **Section 16:** a slice 6 bullet with the above.
  - **Section 4.4's `plumcraft-media` row:** built, with the features `images`, `svg` and `spectrogram`. PDF and mesh are not yet. Arrays become pictures in the engine SDK, so the crate decodes renditions rather than mapping arrays. `probe` and `diff` are still in the core.
  - **Section 6, life cycle and memory:** the decoded caches have a budget and `drop_caches`, waiting for `lifecycle.memory`.
  - **Section 10, the "Main-thread stall, ever" row:** values are met; the open items are listed above.
  - **Section 4.1, the `watched.rs` (`Fulls`) row:** replaced by the service's weak index.
- **`docs/shell-interface.md`:**
  - **10.1:** `plum_blob`'s status as built:
    - `PLUM_PENDING` is 1;
    - unreadable is `PLUM_ERR_UNREADABLE` (-5), since the existing statuses are negative;
    - refused (3) is not built;
    - `plum_bytes` still has no `w`, `h`, `stride` or `format` (that would change the ABI version); a shell gets them from `media.get` or the feed instead.
  - **10.2:** derived keys as built: `derive:rgba:<h>`, `derive:image:<h>@<px>`, `derive:svg:<h>@<px>` (the long side, not `@<w>x<h>`), `derive:peaks:<h>` (f32 pairs), `derive:numbers:<h>`. `derive:pdf`, `derive:spectrogram`, `derive:compare` and `derive:mesh` are not built.
  - **6.3:** the new methods `media.get {handle, want?, px?}` and `media.stats`.
  - **7.1:** `plum_poll`'s `media` item.
  - **4.1:** the config key `media: {cache_mb, workers, queue}`.
  - **5.4:** the media workers.
  - **Appendix A:**
    - `media.rs`, `audio.rs`, and `run_view.rs`'s `Thumbs`: decoding moved; textures stay.
    - `watched.rs`'s `Fulls`: replaced.
    - `diff.rs`: still in the core.
- **`docs/control-protocol.md`:** no change. No reply changed: `ui.inspect` keeps its keys and gains none. Only the timing of when `watched.full`, a pop-out's `full` and pictures become true changes, by a frame or two.
- **`docs/BUGS.md`:** nothing from this slice. It had an uncommitted change from someone else in the tree (B27 and B28 notes), which I left alone.
- **`AI.md`:** at commit time. The `shrink` overflow is a pre-existing defect this slice's hostile sweep found and fixed.
- **`docs/guide.md`:** done. The crate table has a `plumcraft-media` row.
