# N4 slice 5: the remaining viewers

Branch `native/n4-viewers` (from `origin/feat/native-n0` with N3 and N4 slices 1 to 4). Nothing committed: the controller reviews and commits.

## First: what appeared or sounded on the Mac, and writes outside scratch

Nothing modal appeared and nothing sounded. Every Mac launch was a scripted one (`--scratch-state`), never activated; pop-outs and the new Messages window are ordered just above their own document window. No audio device was opened by any automated run: the audio viewer runs its clock silent whenever the app was launched with `--scratch-state` or `--status-file` (every script, the conformance runs and the UI tests), and the simulator runs used the same flag; no offline render was needed because the engine is never created. The document windows of the picture scripts and `conformance.sh` were on screen behind the maintainer's windows while they ran, as in earlier slices. The maintainer's apps (PIDs 5861, 30260, 58303) were never signalled; nothing was built into `native/apple/PlumCraftMac/build` or touched under `claude-debug/native-n3/s6-try/`.

Writes outside scratch: none to the real store or preferences (before and after below). The source tree, the iPad project's own `native/apple/PlumCraftPad/build` (allowed), and the kit's `.build`.

## The maintainer's defect, first: "No way to close popout or get back to the main canvas"

Cause: slice 3 opened a pop-out as a scene of its own whenever the graph's scene was not full screen. On iPadOS 26 with windowed apps the graph's scene is never full screen, so every value opened a new window, which the system showed maximised over the graph; closing it destroyed the scene but nothing brought the graph's scene back to the front, and a stale scene restored after a restart could leave a document browser in its place.

Fixed, on the iPad:

- The sheet with medium and large detents is the default for every pop-out (slice 3's decision 3, answered by the maintainer's use). At medium the graph stays in sight above it.
- Done in the sheet's bar closes it at once, whatever the core is doing, and tells the core after (`view.close`); a swipe down does the same.
- A window of its own only when the person asks (Open in New Window in the sheet's bar), with a Close button in its own bar that destroys the scene (`requestSceneSessionDestruction`), brings the graph's scene back to the front (`activateSceneSession` of the graph's session) and tells the core; closing it with the window's own controls brings the graph back too.
- The parameters inspector's Done now closes at once too; the Settings sheet already had Done and swipe-to-dismiss.
- On the Mac: the pop-out window has its standard close button and Command-W (File › Close, `performClose`), each tells the core (`view.close`), and the document's window is brought back to the front for a person at the app. A pop-out over a full-screen document is `fullScreenAuxiliary` and has its close button.
- Tests through the person's path: the iPad UI test `testWindowsAndNames` now taps the graph after Done and checks it took the tap (passed); `testEveryViewerByATap` opens each kind of value by a double tap on its pin, waits for the sheet and the drawn viewer, taps Done, and taps the graph.

## Inventory

| Kind (media, core type) | egui viewer and its tools | Apple viewer now | Missing |
|---|---|---|---|
| IMAGE (`image`) | `picture`: Fit, 1:1, −, + (`viewer.zoom`); scroll and drag zoom and pan (local); hover readout and click-to-pin probe (`viewer.probe`) from the file's own numbers; scroll-in past the pixels asks for detail; checkerboard alpha | Native zoomable view in each app: the file decoded on the core's workers (up to 2048 a side, more on zoom, at most 4096), Fit and 1:1 (`viewer.zoom`), channel (`viewer.picture`: all, colour, R, G, B, alpha), readout under the pointer (Mac) or a tap (iPad) from the core (`viewer.data probe`), click or tap pins `viewer.probe`, detail asked once on zooming past the pixels | No checkerboard behind alpha; 16-bit and float files show 8-bit pictures (the probe reads their own depth); no −/+ buttons (pinch and Command-scroll) |
| SVG (`vector`) | `svg`: drawn at the zoomed size, Fit, −, + (local) | The same native view, drawn by the core at 2048 and again at 4096 on zooming in; its source offered as `text.plain` ("Source") | No redraw beyond 4096 |
| ARRAY (`ndarray`) | `array.pictures` (views, stats, histograms, view picker `run.view`, zoom, probe); `array.grid` (format presets, colour by value, slice picker); `array.plot` for (N, k) tables | One view: the native picture view (detail by `viewer.zoom detail`, probe); several: a SwiftUI grid of the views; `array.grid` a SwiftUI table paged by the core (`viewer.grid` plane and page, 64 columns at a time), the dtype, shape and channel statistics | Histograms, the view picker (`run.view`), grid formats and colour by value, `array.plot` |
| TABLE (`table`), or rows | `table.grid`: click a header to sort (local), colour by value, resizable columns | SwiftUI `Table` (an NSTableView on the Mac): typed columns, numbers right-aligned, sort by a header through the core (`viewer.table`), 500 rows a page (`viewer.table page`), selectable cells | Colour by value |
| JSON (`tree`) | `yaml` (Copy); `tree` (collapsing headers, Raw JSON) | `yaml` (the core's YAML) first, as the maintainer asked; `tree` an outline of disclosure groups opened and closed by the core (`viewer.tree`), Collapse All, Copy Value of a member (`value.copy {pointer}`) | Raw JSON toggle |
| AUDIO (`audio`) | Play/Stop, Loop, a click seeks while playing, playhead, waveform (overview on the UI thread), spectrogram (B36) | Play and Pause, seek by a tap or drag anywhere, Loop, level, the time, a playhead (`viewer.audio`); waveform from every sample and the spectrogram made on the core's workers (`media.get waveform`, `spectrogram`); AVAudioEngine fed 65,536 frames at a time from the lent samples | No device picker |
| TEXT (`text`) | `text.plain`: Wrap, Find, line numbers | Slice 3's viewer: Wrap, Monospaced, Show All (`viewer.text`) | Find, line numbers, encodings (UTF-8 lossy, as egui) |
| Scalars, enums (`scalar`) | `scalar` | Slice 3's viewer | None |
| Lists (`items` > 1) | `items` (the sheet), item stepper (`run.item`), Sheet toggle | The bar's Previous, "item i of n", Next (`run.item`); `items` a SwiftUI grid of the sheet's thumbnails (tap: `viewer.sheet {on: false, index}`) | Not exercised by a test: no compiled-in engine makes a list with a sheet |
| Bytes (`hex`) | `hex` (virtualised, no paging) | SwiftUI, paged 4,096 bytes at a time by the core (`viewer.hex`) | None |
| Pack-opaque (`x.<pack>`), `bytes`, and `pdf.pages`, `doc`, `xml`, `mesh` | `hex`, or their own viewers | What it is: type, media type, size, summary, the viewer offered, and the engine's picture if it sent one; never a guess at the bytes; `hex` offered | `pdf.pages`, `doc`, `xml`, `mesh`, `array.plot` as viewers of their own |
| Compare | `compare_view.rs` | Not built | All (below) |

Every core type has a viewer other than bytes in both apps; no value ends at "no viewer yet".

## What was done, item by item

1. **Inventory:** above.
2. **Viewers in §8's order:** picture in full (channels, readout, pinned probe, detail on zoom), `items`, `array.pictures`, `table.grid`, `array.grid`, `audio`, `svg`, `tree` and `yaml`, `hex`; `array.plot`, `pdf.pages`, `doc`, `xml` and `mesh` fall back to the engine's picture with what the value is.
3. **Banner items:** the Messages window is built (Mac window, iPad sheet; `messages.get`, `view.windows.messages`). Compare, the Cache Report, the graph's Parameters window and value editors are not.
4. **Faces:** waveform thumbnails drawn from the engine's peaks (`media.get peaks`), on a worker, in the theme's waveform colours. egui draws no table preview on a face, so none was added.
5. **Export:** View As, Copy, Share… and Save As… on every viewer's bar (`value.copy`, `value.share`, `value.saveAs` through `ui.invoke {params, by: person}` and the existing asks); `value.export` writes the file a drag would carry. Dragging out of a pop-out is not wired in either app.
6. **Conformance and tests:** below.

## Viewer commands

| Command | Params | State in `popout.get`'s `state` |
|---|---|---|
| `viewer.table` | `{popout, sort?: column \| null, desc?, page?}` | `table: {sort, desc, page, pageRows}` |
| `viewer.tree` | `{popout, expand?: pointer, collapse?: pointer, collapseAll?}` | `tree: {expanded}` |
| `viewer.audio` | `{popout, play?, at?: seconds, loop?, level?: 0..1}` | `audio: {playing, at, loop, level, seq}` |
| `viewer.picture` | `{popout, channel?: rgba \| rgb \| r \| g \| b \| a}` | `picture: {channel}` |
| `viewer.grid` | `{popout, plane?, page?}` | `grid: {plane, page, pageRows}` |
| `viewer.hex` | `{popout, page?}` | `hex: {page, pageBytes}` |
| `value.copy` | `{popout, pointer?}` | none (raises `clipboard.set` when invoked) |
| `value.export` | `{popout}` | none (a file) |
| `value.share` | `{popout}` | none (raises `share` when invoked) |
| `value.saveAs` | `{popout, path}` | none (raises `file.save` through `run.save` when invoked) |
| `viewer.data` (a method, not a command) | `{popout, part, row?, col?, cols?}` | reads `table`, `tree`, `yaml`, `grid`, `hex`, `probe` |
| `messages.get` (a method) | `{}` | the Messages window's list |

Existing commands now used by the Apple viewers: `viewer.zoom`, `viewer.probe`, `viewer.text`, `view.viewer`, `run.item`, `viewer.sheet`, `view.messages`.

**Where the line is:** whatever changes what is shown is a command (a sort, a page, a member opened, play, pause, a seek, the level, loop, a channel, Fit and 1:1, the viewer, the item of a list, detail). Continuous motion inside a viewer is the shell's: scrolling, a pinch or scroll zoom of a picture, a column's width, and the playhead moving while playing (the core holds where play started and `seq`, which rises on every start or seek, so a script reads what was asked and the shell restarts its clock from it). This matches the canvas, whose pan and zoom are the shell's.

## Large and malformed values

- **Never on the main thread:** tables, sort orders, JSON, waveforms and spectrograms are new derived kinds of the media service (`crates/media`: `table.rs`, `audio::Samples`), made on its workers and cached under its byte budget; `viewer.data` answers `{status: "pending", wait: key}` at once and the shell waits for the feed's `media` note (the kit's `PopoutModel.data`). A sound's samples are read where they are (`Samples`), so a waveform or spectrogram of an hour copies nothing; playback is fed 65,536 frames at a time from the lent bytes.
- **A TABLE is read without a JSON tree of the whole** (a streaming deserializer into typed cells; text cells cut at 4,096 characters; at most 4,096 columns, 50 million rows, 512 MiB). JSON for an outline is refused past 64 MiB with "see it as text".
- **Measured** (scripted runs, `ps -o rss`): a 999,990-row table (26 MB of TABLE JSON) on the Mac: the first page ready 0.7 s after it was asked for; the app's resident memory 959 MB with the text engine in process holding the value too. On the iPad simulator: 999,990 rows shown, paged to page 1,001 of 2,000 and sorted; the app's resident memory 314 MB before and 844 MB after (the in-process engine's copy included). The text engine refuses a CSV of more than 1,000,000 rows (`csv: more than 1000000 rows`); the pop-out of that failed node says so. Kit test: a 50,000-row table read and sorted while a main-actor ticker kept running.
- **Malformed:** a value read as what it isn't comes back `{status: "failed", error}` (a text as a table: "not a table…", a torn array: "N bytes is too few for…"); a failed node's pop-out says "This value failed: …"; hostile parameters are refused. The hostile sweeps cover the new decoders (`crates/media/tests/hostile.rs`: hostile blobs through `Samples`, waveform, spectrogram and `Table::from_bytes`; deep and wide JSON; every new `Want` through the service; derived keys parsed and refused). One conformance case reads values as what they aren't and checks the core still serves.

## Still on the "isn't available yet" banner

Value editors (`node.editor`), the Cache Report (`run.cacheReport`), the graph's Parameters window (`view.params`), Compare (`view.compare`), Attach… (`engine.attach`) and Install Engine… (`engine.install`). Removed this slice: the Messages window.

## B36

Fixed for the Apple shells (the spectrogram is made by the media service) and for egui: `AudioView::sync` now always computes the spectrogram on its own thread. Still on the egui editor's thread: decoding the clip (`Clip::from_blob` copies the samples), the waveform overview when the width changes, and Play's resampling. B36 in `docs/BUGS.md` is not edited (not mine to edit).

## Core, C ABI, kit and contract changes

- **plumcraft-media:** `Want::{Waveform(bins), Spectrogram, Table, Sorted(column, desc), Json}`, `Made::{Table, Order, Json}`, `derived_key` and `parse_derived` (moved from the FFI), `table.rs`, `audio::Samples` (`peaks`, `spectrogram_of`), `audio::ramp`; a sort reuses a table already read.
- **plumcraft-session:** `viewer_state.rs` (`ViewerState` on every pop-out; the six viewer commands).
- **plumcraft-editor-core:** `viewers.rs` (`offered`, `NATIVE`, `number`, `popout_extra`, `viewer.data`), `export.rs` (`value.*`, CSV, float WAV, `.npy`), `popout.get` extended, `values::default_viewer` from the registry, canvas commands `value.copy`, `value.export`, `value.share`, `value.saveAs`, `ui.invoke {params}`, `messages.get`, `Windows.messages`.
- **plumcraft-ffi:** `media.get` takes `want: waveform | spectrogram`; `plum_blob` lends a sort order as u32s and refuses a table or tree. **`PLUM_ABI_VERSION` unchanged (1):** no C function added or changed.
- **plumcraft-array:** Array to Audio and Audio to Array (each the other's inverse, `converts` marks), and the engine declares AUDIO: every core type can now be made on the iPad, which has no audio engine.
- **plumcraft-ui (egui):** B36; a test holds the core's registry to egui's.
- **Kit:** `PopoutValue` fields, `PopoutModel.command/choose/data/takeOut/exportFile`, `DocumentModel.invoke(params:)`, `Pictures.load(want:px:)`, `lend`, `keep`, `Windows.messages`; `PlumCraftShell/Viewers.swift` (the SwiftUI viewers, `ViewerBox`, `PictureSource`, `PicState`, `SoundPlayer`, `MessagesModel`, `MessagesView`); waveform thumbnails in `LayerCanvas`.
- **Apps:** Mac `Popouts.swift` (routing, the picture view's tools, `ProbeImageView`, `MessagesWindow`); iPad `Popouts.swift` (sheet by default, Close and the graph forward, routing, picture tools, `PadMessages`, an anchor for any popover), `Parameters.swift` (Done at once), `GraphViewController.swift`, sample "Viewers".
- **Contract:** `docs/shell-interface.md` 12.14 (iPad sheet by default) and new 13.2.1; `docs/control-protocol.md` (`ui.invoke {params}`, `popout.get` fields, `viewer.data`, `value.*`, the viewer commands); `schema/shell1.json` regenerated last (`Windows.messages`).

## Conformance

49 cases (eight new: a table sorted and paged; a JSON outline; a sound played, sought, waveform and spectrogram; an array's pictures and grid; a picture's channel, probe and detail; an SVG and its source; values read as what they aren't; the Messages window), each popping its value out by a person's double tap (`ui.gesture`).

| Driver | Result |
|---|---|
| The core's own document (Rust) | passes, no skips |
| C ABI headless, and over its control port (Rust) | 49 cases: the text engine's pass, the array engine's (six) pass on a core opened on it; no failure |
| The egui editor (Rust) | the same split, no failure |
| The Swift kit, and over the port (`swift test`) | passes; the read half skips only the array engine's cases, which pass on a core opened on it |
| The Mac app (`conformance.sh all`) | 49 of 49, 0 skipped |
| The iPad app in the simulator (`conformance.sh all`) | 49 of 49, 0 skipped (the final run; two earlier runs crashed the app, below) |
| The iPad UI tests (`conformance.sh gestures`) | 4 of 4 (`testEveryViewerByATap` new) |

No `only:` mark was needed; the new cases that need an engine say so with `needs` (`engine:text`, `engine:array`), as the existing ones do.

## Tests

- `cargo nextest run --all-features --no-fail-fast`: 1,531 of 1,532 passed, 17 skipped; the one failure is the known local-only `plumcraft-craft::params_coverage::hidden_params_match_the_baseline`. None of the known intermittents failed this run.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean. `cargo fmt --all --check`: clean (after a `cargo fmt` of one import line in `crates/ui/src/audio.rs`, made after the gate's clippy and nextest: formatting only).
- `scripts/check-hash-gates.sh` and `scripts/check-core-toolkit-free.sh`: pass (`plumcraft-media` gained `serde`, no toolkit).
- Python (under the scratch environment, after cargo): 224 passed; ruff clean.
- `swift build` clean; `swift test`: 81 tests, all passed (two new: a 50,000-row table paged and sorted off the main thread; an outline and a malformed value).
- Builds: the xcframework; the Mac app (Release) with no code warning; the iPad app for the simulator (Debug and Release) with no warning; for a device (arm64, unsigned, Debug and Release; never installed) with no code warning (Debug prints five of Xcode's "Conditional compilation flags do not have values in Swift" for `_LIBCPP_HARDENING_MODE` on the kit's package targets, a build-setting notice of the `-target` build, and both print the App Intents metadata notice).
- The schema exported last: `crates/editor-core/schema/shell1.json` (`Windows.messages`).

## Store and preferences

Every script of this slice starts by sourcing `claude-debug/native-n4/s5-env.sh`, which refuses to go on when `COMMONKIT_STORE_ROOT` is unset or under `/Volumes/<store>`; the Python stages check it themselves too. Listings: `claude-debug/native-n4/s5-store-before.txt` and `-after.txt`; checksums `s5-prefs-before.txt` and `-after.txt`.

- **Preferences and trusted files:** unchanged (`prefs.json`, `trusted.json`, `com.plumcraft.mac.plist` checksums identical).
- **The real store:** nothing was added. Things were removed: at 21:56 `.hello` lost its eight `e-*.nonce` files and `plummemo/ea/` went (slice 4's leftovers, which the maintainer was to decide on), and at 22:14 `plumz/5a/` and one entry under `plummemo/ab/` went. None of this slice's processes pointed at the real store (each launch had a scratch root, checked); the maintainer's three PlumCraft apps, which use the real store, were running throughout, and the removal of slice 4's leftovers looks like someone's cleanup. Reported, not explained further.

## Processes and simulator

- `pgrep -lx` for `PlumCraft`, `PlumCraftPad`, `CanvasSpike`, `CanvasSpikeMac`, `xctrace` at the end: `5861 PlumCraft`, `30260 PlumCraft`, `58303 PlumCraft` (the maintainer's, there before and untouched); none is mine.
- Simulator: the "iPad Pro 13-inch (M5)" left booted; `com.plumcraft.pad` and the UI test runner uninstalled; nothing erased. The physical iPad was not used.
- Removed my own build folders: `claude-debug/native-n4/s5-build/mac` (547 MB) and `s5-build/device` (709 MB). Disk: 33 GB free at the end (the controller removed `target/debug/incremental` mid-slice).

## Pictures

`claude-debug/native-n4/s5-shots/`: Mac light (`mac-*-light.png`: canvas with faces, table, table sorted, JSON as YAML and as an outline, picture, its red channel, 1:1, an array's picture and grid, SVG and its source, audio with waveform and spectrogram, a malformed value, bytes); iPad light and dark (`pad-*`: the same set, and `pad-table-million-rows-dark.png`, page 1,001 of 2,000 of 999,990 rows sorted); the UI test's `n4s5-ui-*` (each viewer opened by a tap). The Mac's dark set and later Mac light runs could not be captured: from about 22:05 `screencapture -l` answered "could not create image from window" for every window (the Mac had been idle for over an hour; the display was presumably asleep or locked). The Mac light pictures are from this slice's build of 21:51, before the last polish (the playhead, picture files decoded from their own bytes, the grid's column widths, the Messages window). Reports: `mac-report-light.json`, `pad-report-*.json`.

## Defects introduced and caught

- The status file lost a viewer's "picture" after its state changed (the pop-out's description was rebuilt), so the UI test waited forever on an array: kept now.
- A pop-out's description key for a viewer's page clashed with its status word: renamed.
- Array to Audio read a 64-column picture as 64 channels: at most 8 columns are channels now.
- The pop-out sheet was see-through on iPadOS 26 at medium height: opaque now.
- The playhead did not move (the canvas was not redrawn with the clock): fixed.
- The conformance suites' "skipped" expectations named one case: they now name every case for the array engine.
- On the iPad, `conformance.sh all` crashed the app twice (22:12, 22:21) with UIKit's "UIPopoverPresentationController should have a non-nil sourceView or barButtonItem set before the presentation occurs", in the net and the settings cases, which passed in slice 4. Every presentation from the graph and from a pop-out now anchors a popover that comes with none at the middle of the graph and logs its class; the final run passed 49 of 49 and the log names the presenter: a `UINavigationController`, which in this app is only the parameters inspector's popover. Why its anchor is gone at that moment (the inspector re-presented while another presentation is being replaced, `presentReplacing`) is not found: the guard keeps the app alive, and the inspector shows from the graph's middle when it happens.

## Decisions for the maintainer

1. **JSON opens as YAML** (your "format as Yaml for display"), with the outline one tap away; or the outline first.
2. **A pop-out on the iPad is a sheet by default,** a window only on Open in New Window (made to fix your defect; slice 3's decision 3 reversed).
3. **Array to Audio and Audio to Array in the array engine,** added so every core type can be made on the iPad (which can't host the audio engine). Keep them, or move them to a pack.
4. **A scripted run's audio is silent by never opening the device** (rather than an offline render): keep.

## What only a person can verify

- iPad: tap a value's pin twice, see the sheet over the graph; tap Done, and the graph takes your next tap; drag the sheet down; Open in New Window, then Close: the graph's window comes back in front. Repeat in Stage Manager and full screen.
- Mac: a pop-out's close button and Command-W, the document window coming back in front; a pop-out over a full-screen document.
- Play a sound on both (the only path the scripts never take: the device opened), its level, loop and seek; Bluetooth output.
- Sort a table by clicking a column header twice; page through a big table; open and close outline members; Copy from a member's context menu.
- Picture: hover readout on the Mac, a tap on the iPad, the channel menu, pinch past the pixels and see it sharpen.
- Share… and Save As… from a viewer's bar (the person's asks: share sheet, save panel).
- The Messages window from the status line.

## For slice 6

- Compare, the Cache Report, the graph's Parameters window and value editors remain; Compare's difference is in the core already (`diff.rs`) and needs a pop-out-scoped pin.
- iPhone's compact layout: the viewers are SwiftUI and the sheet already has detents; a SwiftUI `Table` shows only its first column in compact width, so the table and grid need a compact form (a list of rows).
- Drag out of a pop-out: `value.export` gives the file; neither app wires the drag yet.
- The iPad popover crash above, if it recurs, now names its presenter in the log.
- `target/debug/deps` is 76 GB; the maintainer approved a `cargo clean` after this slice.
