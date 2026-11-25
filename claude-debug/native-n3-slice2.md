---
title: "N3 slice 2: the Swift kit"
date: 2026-11-20
---

# N3 slice 2: the Swift kit

Branch `native/n3-swift-kit`, cut from `origin/feat/native-n0`. Nothing is committed; the changes are in the working tree for review.

## Summary

All six items were done.

1. **The FFI wrapper** covers the whole header: lifecycle, JSON requests, the feed and `wake` (coalesced, never blocking the core thread, parsed off the main actor), blobs, and every typed hot call. Errors are thrown `PlumError`s with the core's words. Swift 6 language mode, zero warnings, no force unwrap, `try!` or `fatalError` outside tests.
2. **Codable models** for everything the brief lists. A Swift test fails on any field the types don't know, in three places: the scene fixtures, a sample of every message type in `shell1.json` (each variant), and live replies from a real core. Unknown words decode to `.unknown`.
3. **`DocumentModel`**, an `@Observable` main-actor model of one document. It applies `scene.get {since}` by nid, with an observable model per item, so a view redraws only the item whose `rev` changed. It also holds the theme, menus, asks, watched value and engine state, and its tests run headless.
4. **`CanvasLink`**, the geometry and gesture plumbing. It reports geometry typed and in JSON (offset, parts), hit-tests, finds wire targets, and sends every `ui.gesture`. Drag moves go through `plum_gesture_move`, and the dragged faces' wire paths come with a reused buffer.
5. **The conformance runner in Swift** has two drivers, one through the kit and one over the control port. The read half passes on both. The picture case passes on an array-engine core; the array engine is already in the xcframework. The whole suite also passes through the kit.
6. **Build:** the xcframework builds all four slices, and the kit compiles for iOS device and simulator. Build products are ignored by git.

**Found and fixed in the core:** after a `scene.get`, an edit reached the C ABI's feed a full idle tick late. At 500 nodes that was 67 to 80 ms after the edit's reply, on every edit, for a shell that fetches on each change. It now takes 21.5 ms (median), and 3.9 ms when the model fetches as soon as its own command is answered. The fix is eight lines in `crates/ffi/src/core.rs`, with a test that fails without it.

**The gate passed, except the known local failure.** nextest (b3) ran 1,461 tests and passed 1,460; the failure is `plumcraft-craft::params_coverage hidden_params_match_the_baseline`. Clippy, fmt and both scripts are clean. Swift: 33 of 33 tests in five runs; the 7 measurement tests are opt-in.

## The package

**Decision: grow `native/apple/PlumCraftFFI` rather than add a second package.** That way the binary target, the wrapper and the kit version together, and slices 3 to 5 depend on one package with no path dependency between packages. The directory keeps its name, so `build-xcframework.sh`, its `.gitignore` and the slice 1 notes stay right.

| Target | What | Depends on |
|---|---|---|
| `PlumCraftFFI` | The xcframework (`binaryTarget`), not committed | |
| `PlumCraftModels` | Pure Swift. `JSONValue`, `JSONParser` (bytes straight to a JSONValue), `JSONValueDecoder` (a `Decoder` over JSONValue, with an optional `KeyRecorder`), `Open<Known>` (an open word), and the contract's types: `Scene`, `Delta`, `SceneReply`, `Face` and its parts, `Wire`, `Net`, `Frame`, `Group`, `Mark`, `View`, `Fit`, `Palette`, `Picked`, `Anim`, `RunShown`, `ThemeReply`, `MenuItem`, `MenuReply`, `ContextMenuReply`, `Ask`, `AskEnded`, `Watched`, `Inspect`, `PaletteItems`, `Hit`, `Located`, `Gesture`, `GestureReply`, `DragBegun`, `DragUpdate`, `FaceReport`, `GeometrySetReply`, `FeedBatch`, `Changed`, `MediaNote` | |
| `PlumCraft` | `PlumCore` (open, close, `request`, typed `request(T.self, …)`, `command`, `send`, `post`, `feed()`, `blob`), `HotCalls` (`hitTest`, `wireTarget`, `wirePaths`, `geometrySet`, `gestureMove`, `HotHit`, `HotDrag`, `WirePaths`, `FaceMeasure`) | FFI, Models |
| `PlumCraftKit` | `DocumentModel`, `ItemModel<T>`, `ItemTable<T>`, `SceneChange`, `EngineState`; `CanvasLink` | PlumCraft, Models |
| `PlumCraftConformance` | `Conformance` (the runner), `KitDriver`, `PortDriver` | Kit, PlumCraft, Models |

Products: `PlumCraftKit` (the three libraries), `PlumCraft`, `PlumCraftConformance`. No third-party code; Foundation, Observation, Synchronization and Network only. Tools 6.2 (for `@concurrent`), Swift 6 language mode on every target, toolchain Swift 6.4 (Xcode 27.1), platforms macOS 15 and iOS 18. About 4,100 lines of sources and 1,300 of tests.

## Threading

| What | Where it runs | Sendable |
|---|---|---|
| The C wake (`plumWake`) | The core's thread. It only yields to an `AsyncStream<Void>` holding one element, so wakes between polls coalesce and the core never waits on Swift | `WakeBox` is |
| The pump | One detached task, not the main actor. It polls until empty, parses each poll once (`JSONParser`), resumes each request with its result, and yields every non-reply part as a `FeedBatch` to each subscriber | `PumpTarget` is |
| `PlumCore` | Any thread or actor. Its only mutable state (pending requests, subscribers, counts) is behind one `Mutex`, held across `plum_request` so a reply can't be routed before its request is waiting | Yes (final class) |
| `request(T.self, …)` | `@concurrent`: decodes on the cooperative pool, so a 5 MB scene never decodes on the main actor | Results are |
| Hot calls | Synchronous, any thread. They read the core's snapshot and never wait for the core thread | n/a |
| `DocumentModel`, `ItemModel`, `CanvasLink` | `@MainActor`. The main actor only applies decoded deltas (5 to 12 µs) and full scenes (0.6 ms at 500 nodes, 2.3 ms at 2,000) | No (main-actor confined) |
| The feed to a model | `core.feed()` gives each subscriber an unbounded stream (asks must not be lost); `DocumentModel` consumes it in a main-actor task | `FeedBatch` is |

A test (`repliesDontWaitForTheMainActor`) blocks the main actor for 0.4 s while 20 requests from elsewhere are answered.

## Item by item

### 1. The FFI wrapper

- **Lifecycle.** `PlumCore(config:)` throws the core's `plum_last_error` text. `close()` is idempotent and also runs from `deinit`; requests still waiting fail with `.closed`, and the feed's streams end. `PlumCore.rawClose(id)` exists for a closed-twice test, which gets `PLUM_ERR_CORE`.
- **Requests.** `request(method, params, doc:)` returns a `JSONValue`. A refusal throws `PlumError(.refused(core: code?), error)`.
- **The feed.** `feed()` returns an `AsyncStream<FeedBatch>`: `changed`, `asks`, `asksEnded`, `media`, `events`, `log`, `dropped`. A broken item costs only itself (tested). Replies are routed by hand, so a reply that won't decode still resumes its request, with an error.
- **Blobs.** `blob(key)` gives `.ready(Blob)` or `.pending`. The bytes are `Data(bytesNoCopy:)`, given back with `plum_release` when the last copy of the Data goes (zero copy, tested).
- **Hot calls.** `hitTest`, `wireTarget` (side as `output: Bool`), `wirePaths` (decodes the three-float header: nid low, nid high, count; a reused buffer, resized and retried if the snapshot grew), `geometrySet` (every pin and row in two flat buffers so their pointers stay put; NaN for "not said"), and `gestureMove`. Kinds and parts are Swift enums with an `unknown(UInt32)`.
- **Golden rule 1, in Swift.** No force unwrap, `try!`, `as!`, `fatalError` or `precondition` in `Sources` (checked by grep). A count from the C side that would run past the buffer stops the iterator instead of trapping. Integer conversions use `exactly:`.

### 2. Decoded models

- **Codable over a parsed tree.** Each poll is parsed once into a `JSONValue`; each reply is decoded from it by `JSONValueDecoder`, with no second parse. Numbers keep their kind: whole numbers stay whole, because the core refuses `0.0` where it wants a count, and a u64 past Int64 stays unsigned.
- **The kit's own parser.** `JSONParser` goes from bytes straight to a `JSONValue`. JSONSerialization plus bridging every NSDictionary and NSNumber cost 56 ms on a 500-node scene and 221 ms at 2,000; the parser takes 8.8 and 35 ms. It agrees with JSONSerialization on the four shared JSON files (tested). It refuses nesting past 128 levels before a cooperative thread's stack overflows (512 did overflow, below).
- **Drift is a test failure.** `KeyRecorder` records every key a type asked for, so `unknownFields(in:)` lists every field the Swift types don't know, nulls included (a re-encode comparison would miss null fields). Three tests use it:
  - `full.json` and `delta.json` decode with nothing unknown;
  - a sampler builds every message type of `shell1.json` the shell reads (`Scene`, `Delta`, `Hit`, `LocateReply`, `GestureReply`, `ThemeReply`, `GeometrySetReply`, `DragUpdate`) with every property present and each `oneOf` variant in turn (12 variants), so a field the fixtures never fill (marks, a `peaks` preview) is checked too;
  - live replies from a real core (`scene.get` full, delta and with the palette open, `theme.get`, `menu.get` and a context menu, `geometry.locate`, `geometry.hit`, five gestures, a wire drag's begin, move and end, `palette.items`, `geometry.set`, `ui.inspect`'s `watched` after a run) decode with nothing unknown.
- **The sampler isn't vacuous.** `theRecorderFindsFieldsATypeLacks` adds a field at the top and one inside a face's header, and both are reported.
- **What the shell sends** (`Gesture`, `FaceReport`, its `parts`) uses only names the schema has.
- **Unknown words.** `Open<Known>` holds a known word or `unknown(String)`. Run state, shown state, view, spot, pin shape, row source, widget kind, wire kind, tone, role, look, ask kind, domain, media status, hit kind and modifier are all open. A footer or preview kind the kit doesn't know is `.unknown`, and a paint with both a token and a colour is the token. Tested.
- **Hostile values.** Every leaf of a face replaced in turn by each of nine wrong values (null, a string, -1, 1e300, u64 max, [], {}, a long array, true): decoding throws or succeeds, never traps. Twenty kinds of bad JSON throw.

### 3. The observable document model

- **Scene.** `ItemTable<T>` per kind holds items by nid, the drawing order (the scene's: by id, bytewise) and id to nid, since removals name ids. Each item is an `@Observable ItemModel<T>`, replaced only when its `rev` or content changes; the table itself is written back only when items come or go. So a SwiftUI view of one face is invalidated by that face alone. `onChange` gives a layer canvas the nids changed and removed per apply, and whether the selection or view changed.
- **Fetching.** On the feed's `changed {scene}` the model asks `scene.get {since}`. Requests made while one is under way coalesce into one more after it. After a command the model sent itself (`run`, `invoke`, `activate`, and a `CanvasLink` gesture that ran commands) it fetches at once rather than wait for the feed: the contract's read-your-writes, 7.3.
- **The rest.** Theme (`refreshTheme`, `color(paint)`), menu bar (`menu.get`, then the feed's `menus` changes), `contextMenu(context)`, asks pending (added from the feed, removed on `asksEnded`; `answer`, `cancel`), the watched value (`ui.inspect` again when a watched face changes or which face is watched does), engine state, the document closing, and media notes by key.
- **Waiting** (`wait(timeout:until:)`, `nextChange(within:)`) re-checks after each feed batch and each fetch, with the clock only as a deadline.
- **Tests.** One node's change updates that face's model alone: the other face keeps its model and revision, and `onChange` names only the first. A tap selects, and the selection arrives. A removed node takes its face and wire with it. The model then equals a fresh full scene item for item. 60 faces come and go in order. Theme tokens resolve, the File and Edit menus are there, and a context menu holds `node.remove`. A `file.open` ask arrives and ends when cancelled. Watched text and scalar values arrive. A second document has its own model, its hot calls named by its number. A picture from the array engine is made by `media.get` and lent by `plum_blob` (w·h·4 bytes).

### 4. Geometry and gesture plumbing

`CanvasLink` (main actor):

- `report([FaceMeasure])` (typed) and `report(json: [FaceReport])` (offset, parts, footer, by id);
- `locate`, `hit` (typed) and `hitJSON`, `wireTarget`;
- `gesture(Gesture)` and helpers `tap`, `doubleTap`, `context`, `marquee`, `dragBegin`, `dragEnd`, `dragCancel`, each with modifiers and `slop`;
- `dragMove` (typed, never JSON), and `wirePaths(dx:dy:)` for the faces the drag's begin named, with one buffer reused per drag;
- `nudge(dx:dy:press:)`, naming each key press so its repeats join one undo step;
- `CanvasLink.slop(points:zoom:)`.

Tested on a real core: a pin, a header, empty canvas, a wire target that fits, wire paths equal to the scene's at no offset and moved by the offset, a face drag (begin, typed move with exact offsets, paths, end as one `edit.move`, the face where it should be), and the JSON form's parts (a title and a menu hit) with an unknown part refused. The whole conformance suite through `KitDriver` uses the typed geometry and the typed drag move in every case that has them.

### 5. The conformance driver in Swift

`PlumCraftConformance` is `crates/editor-core/src/conformance.rs` in Swift, case for case: steps, `only`, `needs`, `$var` substitution, `$locate`/`$plus`, `until`/`timeout`, every matcher (`$len`, `$exists`, `$contains`, `$deep`, `$not`, `$one`, `$find`, `$min`), both checks, the clean-up after each case. The cases are read from `crates/editor-core/fixtures/conformance/cases.json` in place; nothing is copied into the package.

| Driver | Cases | Passed | Skipped, and why |
|---|---|---|---|
| `KitDriver`, text engine, read half (N3's gate) | 12 | 11 | "a watched picture arrives as picture": needs `engine:array` |
| `KitDriver`, array engine, the cases needing it | 1 | 1 | none |
| `PortDriver` (the core's control port, JSON lines, `auth` first), read half | 12 | 11 | the same picture case: the port's core is on the text engine |
| `KitDriver`, text engine, the whole suite | 24 | 23 | the same picture case |

`aCaseThatIsWrongFails` shows the runner isn't vacuous. Eight wrong steps each fail their case at step 2 with the expected words: a wrong want, a wrong `ok`, a gesture refused, a missing pointer, a `$locate` of nothing, a `save` of nothing, `$len`, a check with too few items. A case needing what the driver lacks is skipped, never passed.

**The array engine** is already linked into the xcframework (the C ABI core opens it by config), so the picture case cost no size. Separating the engines would need a feature per built-in engine (N1 slice 2's note).

**How the read half is run:** `swift test` in `native/apple/PlumCraftFFI` (`PlumCraftConformanceTests`). Each test prints a `CASE driver | pass/skip | name | steps` line per case.

### 6. Build

| What | Time | Size |
|---|---|---|
| `build-xcframework.sh`, all four slices, incremental after the core change | 32 s (each slice's `cargo build --release` about 10 s) | xcframework 426 MB: macOS arm64 + x86_64 215 MB, iOS 106 MB, iOS simulator 105 MB |
| Same, macOS arm64 only (`PLATFORMS=macos MAC_TARGETS=aarch64-apple-darwin`) | 4 to 12 s | 105 MB |
| `swift build --build-tests`, clean | 6.9 s | `.build` 1.5 GB |
| `xcodebuild` of `PlumCraftConformance` (all kit targets), generic iOS device / iOS simulator | 3.9 s / 4.2 s | derived data 387 MB, removed |
| Target directories, unchanged from slice 1 | | aarch64-apple-darwin 826 MB, x86_64 889 MB, ios 821 MB, ios-sim 820 MB, universal 215 MB |

A cold build was not timed this slice (slice 1: about 2.5 minutes). `native/apple/PlumCraftFFI/.gitignore` already ignores `PlumCraftFFI.xcframework/`, `.build/` and `.swiftpm/`. Free disk: 42 GB at the start, 36 GB at the end. The build script changed only in its comment (the library holds both in-process engines; the kit is the package beside it).

## Measurements against the budgets

These are release builds: the core is release, and Swift runs under `swift test -c release`. The machine is an Apple M4 Max. Numbers are medians of three runs (each itself a median of 10 to 40 trials). Measured with `PLUMCRAFT_MEASURE=1 swift test -c release --filter Measure`. Logs: `claude-debug/native-n3/s2-measure-final-run{1,2,3}.txt`, `s2-measure-hot-run*.txt`, `s2-measure-memory-alone.txt`, `s2-timeline-after.txt`; before the core fix, `s2-measure-after-run*.txt` and `s2-measure-run1.txt`. Graphs come from `spike.scene` (n nodes, 1.6 n wires).

| What | 500 nodes | 2,000 nodes | How taken |
|---|---|---|---|
| One-node delta, `node.set` sent until the model applied the face (through the feed) | 21.5 ms (78 ms before the core fix) | 27.4 ms (32.5 before) | `onChange` timestamp minus send time; the model learns of it from `changed {scene}` |
| Same, through the kit's `run` (it fetches when answered) | 3.9 ms | 28.9 ms | as above |
| The core's `node.set` reply alone | 1.7 to 3.6 ms | 18 to 27 ms | timeline probe |
| Of which: `scene.get {since}`, request to parsed reply | 1.9 ms | 6.1 ms | the model's own fetching stopped |
| Of which: decode (1,849 bytes) | 0.035 ms | 0.043 ms | `JSONValueDecoder` |
| Of which: apply on the main actor | 0.005 ms | 0.012 ms | `DocumentModel.apply` |
| Full scene: bytes | 1.26 MB | 5.08 MB | |
| Full scene: request to parsed reply | 21.6 ms | 87 ms | includes the core's build |
| Full scene: parse, kit parser / through JSONSerialization | 8.8 / 55.7 ms | 35 / 221 ms | same bytes |
| Full scene: decode from the parsed tree / JSONDecoder from bytes, for comparison | 20.3 / 24.6 ms | 80.5 / 97.2 ms | |
| Full scene: apply into a new model / again unchanged (main actor) | 0.58 / 1.77 ms | 2.25 / 7.1 ms | |
| First full scene into a started model | 72 ms | 191 ms | `start()` until every face is there |
| Hit test on faces, as the Rust measurement aims / anywhere (often empty canvas) | 0.45 / 2.3 to 2.7 µs | 1.69 / 9 to 12 µs | per call, 20 rounds of 2,000 |
| Wire target | 0.31 µs | 1.23 µs | |
| Drag move (`plum_gesture_move`) | 0.05 µs | 0.05 µs | a 20-face drag |
| Wire paths, 20 faces rotating through the graph / the first 20 by id (520 and 2,020 wires) | 9.6 / 71 µs | 9.7 / 268 µs | buffer reused |
| `plum_geometry_set` of every face: the call / until the snapshot has it | 60 µs / 28 ms | 255 µs / 41.5 ms | the snapshot polled directly |
| Geometry to snapshot, the core quiet first (probe) | 4 to 23 ms, by run | | depends on where the 16 ms throttle stands |
| Model memory: parsed tree, decoded scene, peak, kept | 8 to 10, 2.1, 11 to 14, 0.8 to 2.4 MB | 30 to 39, 8 to 14, 51 to 57, about 9 MB | `malloc_zone_statistics` around parse, decode, apply and release, core idle |
| Request latency (`ping`, `node.move`) | 26.5 µs, 26.7 µs | | demo graph |
| 64 MiB blob borrowed | 53 to 90 µs, zero copy | | |

**Against plan section 10:**

| Budget | Bears on it | Met? |
|---|---|---|
| Pan, zoom, node drag every frame (8.3 ms at 120 Hz), 500 nodes, 800 wires | A drag frame's boundary work is one `dragMove` (0.05 µs) and one `wirePaths` (10 to 71 µs) | The kit's share, yes (under 1% of a frame). Drawing isn't built yet |
| A command to its scene change on screen, 16 ms | One-node delta | At 500 nodes, yes through the kit's own commands (3.9 ms before drawing). No for changes the shell learns from the feed (21.5 ms: the core's 16 ms throttle plus a build). At 2,000 nodes, no (27 to 29 ms): the core's `node.set` alone takes 18 to 27 ms there. The canvas row is read at 500 (the maintainer: under 500 active nodes) |
| Launch to a usable window, under 1 s | First full scene into the model | Yes for the kit's share: 72 ms (500), 191 ms (2,000), off the main actor but the apply |
| Main-thread stall, never over 100 ms | Main-actor work per delta and per full scene | Yes: 5 to 12 µs a delta; 0.6 / 2.3 ms a full scene (7.1 ms re-applied unchanged at 2,000) |
| A preview one frame after decoding | Zero-copy borrow | Consistent: 53 to 90 µs for 64 MiB; a picture from the array engine borrowed |
| Run progress, memory on a warning | | Not measured here (core behaviour) |

## Every change to Rust, the C ABI, the control port and the contract

**C ABI behaviour (one):** an edit's scene change reaches `plum_poll`'s `changed` sooner. Before, a `scene.get` answered in a turn in which the core took no look of its own left the scene marked "published this turn" for later turns. The look after the next edit then took the old generation for the edit's scene, saw nothing new, and the change waited for the next idle look: a 50 ms tick plus a build, 67 to 80 ms after the edit's reply at 500 nodes. Now the mark lasts one turn (`crates/ffi/src/core.rs`, `Worker::run`, eight lines). The feed's content is unchanged, only its timing; no export, struct or JSON shape changed, and `PLUM_ABI_VERSION` stays 1.

- **Test:** `crates/ffi/tests/scene.rs` `an_edit_reaches_the_feed_without_waiting_for_a_tick`. Fifteen edits, each fetched at once as a shell does; the median edit-to-change must be under 35 ms. Without the fix it fails with a median of 77.8 ms (`[67.3 … 80.5] ms`); with it, it passes.
- **Who it affects:** the C ABI only. The egui editor's control port has no feed.
- **Not changed:** the control port's replies and the egui editor.
- **Measured afterwards:** geometry-to-snapshot is the same with and without the fix (28.4 vs 28.4 ms at 500, 41.5 vs 41.5 ms at 2,000).

**Contract (`docs/shell-interface.md`):**

1. 7.1: the fix above, as built.
2. 6.1: as built, `code` is sent only for refusals of a request's `doc` (`unknown_doc`, `bad_params`); every other refusal, an unknown method included, has `error` alone, so a shell treats `code` as optional. Found by the kit's hostile test, and documented rather than changed (adding `code` everywhere would change recorded replies on every shell's control port).
3. 12.4: a context menu's reply is `{context: <kind>, items}`, not 12.1's `{gen, menus}`. That was so since N1 slice 8 and wasn't written; the kit first decoded it wrong.
4. 16.1: the Swift kit's runner and its two drivers.

`docs/control-protocol.md` is unchanged (the change is the C ABI's feed, which the control port lacks).

**Rust otherwise:** none. No schema change; `shell1.json` is unchanged.

**Docs:** `docs/research/24-native-shells.md` section 18 gains slice 2; `docs/STATUS.md` gains an entry. `AI.md` and `docs/BUGS.md` were not touched.

## Tests

| Suite | Result |
|---|---|
| `swift test` in `native/apple/PlumCraftFFI`, five runs | 33 of 33 each time; 7 measurement tests opt-in (skipped). No failure, nothing intermittent |
| PlumCraftModelsTests (no core) | 13: fixtures (3), schema (2), hostile (2), values (6) |
| PlumCraftTests (a real core) | 15: core (5), hot calls (2), document model (7), live replies (1) |
| PlumCraftConformanceTests | 5: read half through the kit, array case through the kit, read half over the port, whole suite through the kit, the runner not vacuous |
| `swift build --build-tests`, clean | 0 warnings (Swift 6 language mode, complete concurrency checking) |
| `cargo nextest run --all-features --no-fail-fast`, b3, fresh folders (`claude-debug/native-n3/s2-gate1-b3.log`) | 1,460 of 1,461 (16 skipped); failed: `plumcraft-craft::params_coverage hidden_params_match_the_baseline`, the known local-only failure. 1,460 before plus 1 new |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `scripts/check-hash-gates.sh` / `check-core-toolkit-free.sh` | ok / ok |
| The maintainer's `prefs.json` and `trusted.json` (SHA-256) | unchanged (`9221fa66…`, `a10d6fc8…`). Swift tests set `PLUMCRAFT_EDITOR_PREFS` to scratch, `COMMONKIT_HASH=b3` and a 2 GB store floor before the first core opens |

The N0 package's 6 XCTest tests became Swift Testing tests. Their checks are kept (open, list, build, run and a 64 MB zero-copy borrow; hostile input and a second close; latency and the 64 MiB borrow as measurements). Their file is `claude-debug/native-n3/s2-n0-PlumCoreTests.swift.orig`; `Raw.swift` was replaced by `PlumCore.rawClose`.

## Defects

**Pre-existing, found by this slice:**

1. **An edit reached the C ABI's feed a tick late after a `scene.get`**, on every edit for a shell that fetches on each change (above). Fixed and tested.
2. **The contract said refusals carry `code`; most don't.** Documented (above).
3. **A context menu's reply shape was undocumented** (above).

**Introduced and caught:**

1. **A u64 past Int64 decoded as -1:** NSNumber's `int64Value` of 2^64 − 1 is −1, and my check took a negative for "fits". Caught by `numbersKeepTheirKind`; the NSNumber's type code now decides.
2. **The schema sampler stopped at depth 12**, so a colour deep in a face sampled as null. Caught by the schema test failing; the limit is now 60. Test code only.
3. **The sampler's "any JSON" fields sampled as a number** (`GestureReply.drag` is a bare value in the schema). Caught the same way; they sample as null. Test code only.
4. **`menu.get {context}` decoded as a menu bar.** Caught by the live-reply test and the context-menu test; `ContextMenuReply` added.
5. **A test sent `node.remove {ids}`** where the command takes `nodes`. Caught by the test failing.
6. **A test expected `code: unknown_method`.** The core doesn't send it (pre-existing 2 above).
7. **The parser's first nesting limit (512) overflowed a cooperative thread's stack** in a debug build: a SIGBUS in the test process. Caught by `deepNestingIsRefusedNotAStackOverflow`. The limit is 128, and the test now also writes, compares, decodes and records a value 128 levels deep.
8. **Two measurement errors.** Geometry-to-snapshot first passed before the geometry landed (the core's 220-wide estimate already hit the point probed), then waited for the feed instead of polling the snapshot. The hit test's random points mostly missed and weren't comparable with Rust's. Caught by comparing with slice 1's Rust numbers; each is now measured as Rust does it, with the other pattern kept and named.
9. **Footprint deltas gave negative memory** (the core's own transient memory). Replaced by `malloc_zone_statistics` around each step with the core idle.
10. **Caught in design, before tests:** writing a delta into a table in place would have notified every observer of the whole table on every delta, because `@Observable` notifies on any mutation. Tables are now written back only when items come or go.

## Decisions the maintainer may want to revisit

1. **One package grown**, keeping the name `PlumCraftFFI` for the directory and package, rather than renaming it or adding `PlumCraftKit` beside it.
2. **Codable over a parsed tree** with the kit's own parser and decoder, rather than Foundation's JSONDecoder from bytes. It parses each poll once, lets several replies in one poll share one parse, and is what makes the drift check exact. Cost: parse plus decode is 29 ms against JSONDecoder's 24.6 ms at 500 nodes, and 116 against 97 ms at 2,000. Both run off the main actor.
3. **JSON nesting is limited to 128 levels.** Deeper is refused as bad JSON, so a widget value nested deeper than that would lose its reply.
4. **The model fetches the scene as soon as its own command is answered**, besides on the feed's change: one extra request per edit, which can be an empty delta.
5. **A feed subscriber's stream is unbounded** (asks must not be lost), so a subscriber that stops reading grows memory.
6. **The watched value is read again** (`ui.inspect`) when a watched face changes or which face is watched does. There is no `watched` domain in the C ABI's feed yet.
7. **The theme isn't refreshed by itself:** the C ABI's feed has no theme domain. A shell calls `refreshTheme()` on an appearance change.
8. **Measurements are opt-in** (`PLUMCRAFT_MEASURE=1`), so `swift test` stays under a second.
9. **`code` was documented as absent** rather than added to every refusal.
10. **The fresh mark now lasts one turn.** The cost is one extra scene build at the next look whenever a `scene.get` lands within 16 ms of the last look.

## For slice 3 (the layer canvas and the Mac app)

- **Open:** `PlumCore(config:)` with `store_root` (the app's caches), `platform`, and in debug builds `control: {port: 0, token_file}`. Then `DocumentModel(core:)` and `start()`; `model.engine` says when the engine connects.
- **Layers by nid:** `model.onChange` gives the nids changed and removed per apply, for a layer per face and a shape layer per wire. Read items with `model.faces[nid]?.item`, `model.wires…`; `faces.order` is drawing order (the last face on top).
- **Measure, then report:** after a face's `rev` changes, `CanvasLink.report([FaceMeasure])` with the face drawn at its position (offset 0). Use `report(json:)` for header parts (title, menu, chevron) and footers, which the hit test uses for parts.
- **Colours:** `model.color(paint)`, from `theme.get`.
- **Fit:** `model.view.fit`, once per `seq`.
- **Palette:** `model.view.palette`, then `palette.items`.
- **Gestures:** send recognised gestures with `CanvasLink`, `slop = CanvasLink.slop(points:zoom:)`. For a drag: `dragBegin`, then each frame `dragMove` (typed) and `wirePaths(dx:dy:)` for `draggedFaces`, then `dragEnd`. Its result is one undo step, and the model fetches the scene once it is answered.
- **Menus:** `model.menuBar` becomes `NSMenu`, refreshed on the feed's `menus`; an item runs with `model.activate(item)`. A context menu is `contextMenu(reply.context)` from a `context` gesture.
- **Asks:** watch `model.asks`; answer or cancel each.
- **Running the read half against the app:** the conformance runner takes any `ConformanceDriver`. For the app, measuring its own faces, `PortDriver` against the app's control port needs `reports` false. It is fixed to true today, a one-line option to add, or a driver over the app's `CanvasLink` with its own geometry.
- **At 2,000 nodes:** a full scene takes about 190 ms to arrive (show "connecting" or a placeholder), and an edit about 28 ms (the core's command cost). At 500 nodes both are inside budget.

## What I did not verify

- **iOS:** the kit compiles for a generic iOS device and the simulator. Nothing linked it into an app, and nothing ran on a simulator or device. The x86_64 macOS slice is built and never run.
- **Linux:** none of it ran there. The Rust change (`crates/ffi/src/core.rs` and its test) passed the Mac gate only. The gate should run on the Linux test host before a merge; I did not run it.
- **That typed replies decode off the main actor** follows from `@concurrent`; no test asserts which thread decodes.
- **The memory numbers** are from the heap's in-use count with the core idle but not stopped. Its idle looks allocate, so they are ranges (one 2,000-node run read −0.3 MB kept).
- **Budgets on a device:** every number is from one Mac (M4 Max). The iPad's numbers are slice 5's.
- **The timing assertion** in `an_edit_reaches_the_feed_without_waiting_for_a_tick` (median under 35 ms) passed every time here. It is timing-based, and a loaded Linux runner could approach the bound.
- **No drawing or view code exists yet**, so "on screen" in every budget above means "in the model".
