# N0(b): the C ABI spike (`plumcraft-ffi`)

2026-11-19. Stage N0(b) of `docs/research/24-native-shells.md`: a hand-written C ABI through which Swift and Kotlin/JVM drive a real PlumCraft session with the text engine in-process. It has asynchronous JSON requests, a payload-free `wake`, a poll that returns replies and events, and borrowed bytes. Nothing is committed.

## Verdict

- **Decision 2 (a hand-written C ABI carrying JSON) is confirmed.** The boundary costs about 7 µs per request from Rust and 25–55 µs per request from Swift or the JVM, including the hop to the main thread. That is under 0.5% of the 16 ms edit budget. The ABI is 10 functions plus 9 JNI shims. One mechanism serves Swift, JNI and Rust, and the same tests pass on macOS, the iOS simulator, the host JVM and an Android emulator.
- **JSON for the scene is confirmed at 500 nodes, with conditions.** A 500-node, 800-wire graph is 96 KB. Requesting it, crossing the boundary and decoding it takes 1.5 ms in Rust, 1.7 ms in Swift (`JSONSerialization`) and 1.5 ms on the JVM. That fits inside a frame, but it should not be resent on every edit. At 2,000 nodes it is 388 KB and 6–7 ms (8 ms more for Swift `Codable`), which is acceptable on open and too much per edit.
  - The scene should go out by revision (changed items only), as section 5.2 already plans. With that, no binary scene format is needed.
  - Two cheap wins were found. The core spends about 1 ms of the 1.5 ms building a `serde_json::Value` tree twice; serialising straight to bytes would cut that to about 0.1 ms. And Swift `Codable` is 3–4 times slower than `JSONSerialization`, so a generated decoder should be measured before it is chosen.
- **The budget actually missed is in the session, not the boundary.** `command.run node.add` takes 3.3 ms on a 500-node graph and **30 ms on a 2,000-node graph**, which is over the 16 ms budget. A profile puts the cost in `plumcraft_session::run::stale_reasons`, which rebuilds each node's incoming wires by scanning every wire (`incoming(g, id)`, twice per node). Every edit is therefore O(nodes × wires). Indexing the wires by target once fixes it. This is for the maintainer: I did not change the session.
- **Borrowed bytes work with no copy, for blobs the store keeps as they are.** Borrowing 64 MiB takes 58–120 µs. Resident memory does not grow at borrow time, and grows by 64 MiB only as pages are read. Swift's `Data` reads at the exact lent address. A value stored deflated (compressible text) must be inflated and verified first: 12 ms for 64 MB on this Mac, 29 ms on the emulator, plus a 64 MB owned buffer. That would stall a frame if `plum_blob` ran on the UI thread, so N2 should resolve blobs on the core and have `plum_blob` hand out only bytes that are already resolved.

## The machine

| What | Value |
|---|---|
| Mac | Apple M4 Max, 36 GiB, macOS 27.0.1 (26A434) |
| Rust | rustc 1.99.0; release builds, no debug info |
| Swift | Swift 6.4, Xcode 27.1; language mode 6 (strict concurrency) |
| JVM | OpenJDK 26.0.2 (Homebrew), Gradle 9.7.1 wrapper, Kotlin 2.4.21, kotlinx-serialization 1.12.0, JUnit 4.13.2 |
| iOS simulator | iPhone 17 Pro, iOS 26.5 (arm64 simulator slice) |
| Android | NDK 30.0.16248370 (Homebrew android-commandlinetools), API 29 target; emulator AVD `pursuit_api35` (Android 15, google_apis, arm64-v8a), headless |
| Store | commonkit, hash b3 (the policy file at `/Volumes/<store>/store/.policy.json`); each test core has its own `store_root` under the scratch root |

## What exists and where

| Path | What |
|---|---|
| `crates/ffi/` | New crate `plumcraft-ffi` (`publish = false`; `staticlib`, `cdylib`, `rlib`; in the workspace through `crates/*`). Its lints repeat the workspace's except `unsafe_code`, and add `clippy::undocumented_unsafe_blocks = deny` |
| `crates/ffi/src/lib.rs` | The C ABI: 10 `extern "C"` entries, each wrapped in `catch_unwind`, plus the only `unsafe` blocks (each with a `SAFETY:` comment) |
| `crates/ffi/src/core.rs` | The core thread, the event pump, the outbox and coalesced wake, and blob lending. Safe Rust only |
| `crates/ffi/src/jni.rs` | JNI shims behind the `jni` feature: JSON as `byte[]`, bytes as a direct `ByteBuffer`, and a wake that never calls into the VM |
| `crates/ffi/include/plumcraft.h`, `module.modulemap` | The hand-kept header (module `PlumCraftFFI`, links `plumcraft_ffi`) |
| `crates/ffi/tests/c_abi.rs` | The ABI called as a C caller would call it: the full flow, the demo run, hostile input, 8 threads × 200 requests, two cores, closing a busy core |
| `crates/ffi/tests/header.rs` | Fails when the header's functions or constants drift from `src/lib.rs` (a textual check) |
| `crates/ffi/tests/measure.rs` | The Rust measurements (`#[ignore]`d) |
| `native/apple/build-xcframework.sh` | macOS arm64+x86_64 (lipo), iOS device, iOS simulator; `target/` per triple; release, no debug info |
| `native/apple/PlumCraftFFI/` | Swift package: `binaryTarget` xcframework (not committed, `.gitignore`), a `PlumCraft` library target (`PlumCore`, `Blob`) and XCTest tests including the measurements |
| `native/android/` | Gradle JVM project (wrapper copied from the native virtimp.com app): `PlumNative.kt` (JNI declarations), `PlumCore.kt` (wrapper), `PlumCoreTest.kt` (host-JVM tests and measurements), `run-jvm-test.sh`, `build-android.sh` |
| `claude-debug/native-n0/` | Logs: build logs, Swift (debug, release, iOS simulator), JVM results XML, emulator runs, Rust measurements, the `node.add` profile |

### Changes to existing code

| File | Change |
|---|---|
| `crates/store/src/lib.rs` | New `Store::map_handle(&Handle) -> Option<Blob>`: maps a plain (not deflated) segment by handle, without reading or verifying it, as `resolve` already maps large arrays. It has one new unit test. |
| `Cargo.lock` | `plumcraft-ffi` added. `jni-sys 0.4.1` was already in the lock (through cpal). |

Nothing else outside `crates/ffi` and `native/` changed. The hash gates pass, and `cargo tree -p plumcraft-ffi --all-features` has no egui, winit or wgpu.

## The C signatures, and why they differ from the sketch

```c
typedef uint64_t plum_core;                        /* a number, not a pointer */
typedef void (*plum_wake_fn)(void *ctx);
typedef struct { uint8_t *ptr; size_t len; size_t cap; } plum_buf;
typedef struct { const uint8_t *ptr; size_t len; uint64_t token; int32_t status; } plum_bytes;

uint32_t   plum_abi_version(void);
plum_core  plum_open(const uint8_t *config_json, size_t len, plum_wake_fn wake, void *ctx);
plum_buf   plum_last_error(void);
int32_t    plum_close(plum_core core);
uint64_t   plum_request(plum_core core, const uint8_t *json, size_t len);
plum_buf   plum_poll(plum_core core);
void       plum_free(plum_buf buf);
plum_bytes plum_blob(plum_core core, const uint8_t *handle, size_t len);
int32_t    plum_release(uint64_t token);
uint64_t   plum_leases(void);
```

| Sketch (section 7) | Built | Why |
|---|---|---|
| `plum_core *` | `uint64_t` core number in a registry | A double close, a call after close or a made-up core becomes `PLUM_ERR_CORE` or a 0, never a use after free. A pointer cannot be checked. |
| `plum_request` returns an id | Same; 0 only when the core isn't open | Bad JSON, a missing method or an unknown method each get an error reply with an id, as the control channel does (`bad JSON: …`). The caller's own `id` comes back as `tag`. |
| `plum_poll` returns replies and changes | `{"replies": [...], "events": [...], "dropped": n}`; an empty buffer when there is nothing | `events` are the engine's protocol-2 envelopes unchanged. Section 7's "changed domains, scene changes, asks, log lines" need the editor core (N1/N2). The empty case costs no allocation. |
| `plum_blob(core, const char *handle)` returning `{ptr, len, w, h, format, token}` | `(ptr, len)` handle; `{ptr, len, token, status}` | Every string crosses as (pointer, length), as JNI and Swift `Data` want. The width, height and format are already in the DataRef the events carry. A `status` says why there is nothing. |
| `plum_release(core, token)` | `plum_release(token)` | Leases are process-wide, so lent bytes stay valid after `plum_close`. A Swift `Data` or a Java `ByteBuffer` can outlive the core, and the old signature would have needed a live core to give them back. |
| `plum_free(plum_buf)` | Same, with `cap` | The buffer is a `Vec` handed over without a copy, and freeing it needs the capacity. |
| (none) | `plum_abi_version`, `plum_last_error`, `plum_leases` | Version check at load time; why `plum_open` failed; a leak check for the tests. |
| Hot path: `plum_wire_paths`, `plum_hit_test`, `plum_wire_target` | Not built | They need the scene and geometry in the core (N2). |

The config is `{"store_root": path, "demo": bool}`. Any `engine` other than `text` is refused with a reason.

Requests are the control protocol's methods. `command.run`, `command.list` and `graph.get` go through `plumcraft_automation::dispatch`, after which the session's outbox (runs, cancels, releases) goes to the engine. Spike-only methods are `ping`, `run` (`run.start`, shorter), `spike.waitRun {timeout_ms}` (replies when the run ends), `spike.scene {nodes, wires}`, `spike.demo`, `spike.publish {bytes, seed}` (pseudo-random bytes into the engine's store, so they are kept plain) and `spike.stats`.

### Threads and the wake

- **Each core has three threads:** `plumcraft-ffi-core` owns the `Session` and does every engine call except the long poll; `plumcraft-ffi-pump` long-polls `events.poll` as `Link` does, without egui; and the engine's own worker.
- **Only the core thread calls `wake`.** An atomic "outstanding" flag means at most one wake goes out until the shell polls. `plum_poll` clears the flag before it takes the outbox, so nothing can be missed.
- **A shell that calls `plum_close` from inside its wake** does not deadlock: `close` does not join its own thread.
- **Swift:** the wake is a file-scope C function that only yields to an `AsyncStream` with `bufferingNewest(1)`. A main-actor task drains the stream. The wake never runs main-actor code, which avoids the trap recorded in section 2.3.
- **JNI:** the wake never attaches a thread to the VM. It signals a condition variable that a Kotlin daemon thread waits on in `nativeAwait`; that thread then posts `drain()` to the shell's main executor. The rule "the wake only schedules" holds on both platforms.

## How to rerun everything

```sh
# Rust: the ABI tests, hostile input, threads, two cores, header drift
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p plumcraft-ffi --all-features
# Rust measurements (release)
CARGO_PROFILE_RELEASE_DEBUG=0 MACOSX_DEPLOYMENT_TARGET=15.0 cargo test --release --target aarch64-apple-darwin -p plumcraft-ffi --test measure -- --ignored --nocapture --test-threads 1

# Apple
native/apple/build-xcframework.sh                     # about 75 s from cold, all slices
(cd native/apple/PlumCraftFFI && swift test -c release)
(cd native/apple/PlumCraftFFI && xcodebuild test -scheme PlumCraftFFI -configuration Release \
   -destination 'platform=iOS Simulator,name=iPhone 17 Pro' -derivedDataPath ../../../target/xcode-derived)

# JVM (host dylib with the jni feature, then Gradle)
native/android/run-jvm-test.sh
# Android .so
native/android/build-android.sh                       # -> native/android/jniLibs/arm64-v8a/libplumcraft_ffi.so

# The C ABI tests on the Android emulator (see "Surprises": the store floor)
B=/opt/homebrew/share/android-commandlinetools/ndk/30.0.16248370/toolchains/llvm/prebuilt/darwin-x86_64/bin
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$B/aarch64-linux-android29-clang CC_aarch64_linux_android=$B/aarch64-linux-android29-clang \
  AR_aarch64_linux_android=$B/llvm-ar cargo test --release --target aarch64-linux-android -p plumcraft-ffi --test c_abi --no-run
adb push <the test executable> /data/local/tmp/plum_c_abi
adb shell 'cd /data/local/tmp && TMPDIR=/data/local/tmp COMMONKIT_STORE_MIN_FREE_GB=1 ./plum_c_abi'
```

Each measurement prints one `MEASURE {json}` line.

## Measurements

Times are medians, with p99 in brackets. "Swift" means a release build of the package through the main actor, and "JVM" means through the waiter thread and a single-thread "main" executor. Each Swift and JVM request is a full round trip: send, wake, hop to main, poll, parse, and complete the continuation or future.

### (i) Request to reply, trivial commands

| From | `ping`, median (p99) | `command.run node.move`, median (p99) | n |
|---|---|---|---|
| Rust, C ABI in-process (Mac) | 6.7 µs (11 µs) | 7.2 µs (10.5 µs) | 5,000 |
| Swift, macOS, `swift test -c release` | 24.5 µs (43 µs); 52 µs (99 µs) in an earlier run | 24.8 µs (37 µs); 51 µs (86 µs) earlier | 2,000 |
| Swift, iOS simulator | 65 µs (1.3 ms) | 50 µs (449 µs) | 2,000 |
| JVM (host), Kotlin | 27.5 µs (66 µs) | 24.6 µs (47 µs) | 2,000 |
| Rust, C ABI on the Android emulator | 73 µs (126 µs) | 74 µs (130 µs) | 5,000 |

The two Swift macOS runs differ by about two times with the same code; I put that down to scheduling and CPU frequency, not the boundary. The emulator figures are an emulator's, not a phone's.

### (ii) JSON for a large scene: `graph.get`

The graph is `spike.scene`: Join and Upper case nodes, wired from earlier nodes only. "End to end" runs from the request to the reply parsed into the language's generic JSON tree.

| | 500 nodes, 800 wires | 2,000 nodes, 3,200 wires |
|---|---|---|
| Bytes in the poll | 95,732 | 387,706 |
| Rust: end to end (serde_json `Value`) | 1.47 ms (1.50) | 5.95 ms (6.10) |
| Rust: of which, decode of the poll | 0.39 ms | 1.52 ms |
| Rust: serialising the `Graph` alone | 0.10 ms | 0.41 ms |
| Swift macOS: end to end (`JSONSerialization`) | 1.74 ms (1.85) | 6.60 ms (7.31) |
| Swift macOS: `JSONSerialization` decode alone | 0.63 ms | 2.12 ms |
| Swift macOS: `Codable` decode alone (typed nodes and wires) | 2.06 ms | 8.06 ms |
| Swift macOS: re-encoding the result to `Data` (what `request(_:) -> Data` adds) | 0.75 ms | 3.05 ms |
| Swift iOS simulator: end to end | 6.0 ms (37) | 19.9 ms (71) |
| JVM: end to end (`JsonElement`) | 1.53 ms (2.12) | 6.08 ms (7.43) |
| JVM: `JsonElement` decode alone | 0.39 ms | 1.10 ms |
| JVM: `@Serializable` decode alone | 0.75 ms | 1.51 ms |
| Rust on the Android emulator: end to end | 2.21 ms | 10.3 ms |
| **Session: `command.run node.add` on that graph (Rust)** | **3.3 ms** | **30.1 ms** |
| Session: `node.add`, Android emulator | 3.8 ms | 33.3 ms |
| Building the scene (`spike.scene`, one undo step) | 16 ms | 285 ms |

- **Where the core's millisecond goes.** End to end minus decode is about 1.1 ms (500 nodes), yet serialising the graph alone takes 0.1 ms. The rest is the path through `dispatch`: `serde_json::to_value(graph)` builds a `Value` tree, `ControlReply` is turned into a `Value` again (a deep clone), then into a string. Serialising straight to bytes would cut most of it.
- **`node.add` at 2,000 nodes** is a session cost. The profile is in `claude-debug/native-n0/node-add-2000.sample.txt`: `stale_reasons`, then `Graph::is_mapped`, then `memcmp` in BTreeMap lookups.

### (iii) `plum_blob` on 64 MiB

The 64 MiB blob is pseudo-random bytes from `spike.publish`, kept plain and so mapped. "RSS" is the process's resident memory before the borrow, after it, and after reading one byte of every 16 KiB.

| From | Obtain | Second borrow (shared) | Touch every page | RSS before / after borrow / after touch (MB) | Same pointer |
|---|---|---|---|---|---|
| Rust (Mac) | 59 µs | 1.3 µs | 2.1 ms | 74 / 74 / 140 | yes |
| Swift macOS | 75–96 µs | 1.7–5.8 µs | 2.0–7.0 ms | 148 / 148 / 214 | yes: `Data.withUnsafeBytes` gives the lent address |
| Swift iOS simulator | 119 µs | 6.2 µs | 3.3 ms | 202 / 202 / 267 | yes |
| JVM (direct `ByteBuffer`) | 285 µs | 20 µs | 2.1 ms | 203 / 203 / 268 | direct, read-only buffer over the lent address |
| Rust, Android emulator | 42 µs | 4.4 µs | 0.8 ms | 10 / 10 / 75 | yes |

For comparison, the text engine's own 64 MB output (`text.repeat`) is stored deflated, because it is compressible. It is therefore resolved through the connection (inflated and checked against its handle):

| From | Obtain | RSS before / after borrow (MB) |
|---|---|---|
| Rust (Mac) | 11–13.5 ms | 203 / 272: an owned 64 MB copy |
| Rust, Android emulator | 29 ms | 137 / 203 |

That proves no copy: obtaining takes microseconds (a 64 MiB `memcpy` alone costs milliseconds), resident memory doesn't move at the borrow, and the shell reads at the lent address. `Data(bytesNoCopy:…deallocator:)` releases the lease when the `Data` goes, and the test checks `plum_leases()` before and after.

### (iv) Wakes and events during a run

The runs are a fan of Upper case nodes from one literal: two status events per node, plus the reply. The runs are short (3–25 ms), so these are burst rates, not steady state.

| From | Nodes | Run (ms) | Events | Wakes | Polls with events | Events per poll, mean (max) | Wakes per second |
|---|---|---|---|---|---|---|---|
| Rust (Mac) | 100 | 3.4 | 203 | 10 | 10 | 22.6 (70) | 2,973 |
| Rust (Mac) | 500 | 23.1 | 1,003 | 14 | 14 | 77.2 (216) | 607 |
| Swift macOS | 100 | 3.3 | 203 | 9 | 9 | 25.4 (76) | 2,699 |
| Swift macOS | 500 | 23.5 | 1,003 | 13 | 12 | 91.2 (224) | 552 |
| Swift iOS simulator | 500 | 31.1 | 1,003 | 22 | 43 | 23.9 (64) | 707 |
| JVM | 100 | 2.9 | 203 | 8 | 8 | 29.0 (90) | 2,726 |
| JVM | 500 | 23.3 | 1,003 | 14 | 14 | 77.2 (172) | 602 |
| Rust, Android emulator | 500 | 36.9 | 1,003 | 38 | 38 | 27.1 (70) | 1,030 |

- Coalescing works: about 70–90 events per wake on the Mac at 500 nodes, and never more than one wake outstanding.
- The rate of wakes is still above a display's (120 Hz) during a fast run. The budget's "one scene change per frame" therefore still has to come from the editor core (N2), which should coalesce the run state into revisions.
- While a node produced 64 MB of text, the largest poll was 16 KB (the excerpt is capped), and the run took 10 polls and 30 KB in total.

## What passed

| Check | Where | Result |
|---|---|---|
| `cargo test -p plumcraft-ffi --all-features` | Mac | 9 passed (6 ABI, 3 header); 4 measurements ignored, run separately |
| `cargo clippy --all-targets --all-features -- -D warnings` (whole workspace) | Mac | clean |
| `cargo fmt --all --check` | Mac | clean |
| `cargo test -p plumcraft-automation -p plumcraft-session -p plumcraft-store` | Mac | all pass, including the new `map_handle` test |
| `scripts/check-hash-gates.sh` | Mac | ok |
| `swift test` (debug and `-c release`): open, list, build by `node.add`/`node.set`/`wire.connect`, run, status events, blob, hostile input, measurements | macOS arm64 | 6 of 6, twice |
| `xcodebuild test` of the same package | iOS simulator, iPhone 17 Pro | 6 of 6 |
| `run-jvm-test.sh`: the same sequence through JNI | host JVM (macOS arm64 dylib) | 6 of 6 |
| `build-android.sh` | arm64-v8a, API 29 | `libplumcraft_ffi.so`, 15 MB; all 10 `plum_*` and 9 `Java_com_plumcraft_ffi_PlumNative_*` symbols exported |
| The C ABI tests (`c_abi`), cross-built and run on the emulator | Android 15 arm64 emulator | 6 of 6 with `COMMONKIT_STORE_MIN_FREE_GB=1`; 5 of 6 without it (see Surprises) |
| Two cores in one process | Mac, emulator | independent graphs, runs and events; one closes and the other goes on |
| Hostile input | Mac, Swift, JVM, emulator | none panicked, crashed or hung (list below) |

The hostile cases covered:

- a null config, and configs that are not JSON, an array, not UTF-8, or name an unknown engine;
- requests that are empty, null with a length, not UTF-8, not JSON, an array, missing a method, an unknown method, `command.run` with a numeric command, or a NUL inside a kind;
- blob handles that are empty, not UTF-8, malformed, unknown, of the other algorithm, null, or on an unknown core;
- releasing token 0, `u64::MAX`, or the same token twice, and freeing an empty buffer;
- a double close, closing core 0, and request, poll or blob after close;
- 8 threads × 200 requests while a run goes on: 1,600 unique ids and every one answered;
- closing while a run is in flight with 100 requests queued: under 5 s.

## What failed, and what I did about it

- **The Android emulator, before the workaround.** One of the six ABI tests failed: the engine's 64 MB value had no handle (`"the store declined the value"`). The cause is commonkit's platform default `min_free_bytes = 8 GiB`, and the emulator's `/data` has 7.9 GB free. Every publish is declined, so on Android nothing is a store blob. I did not change the test or the backend. I reran with commonkit's own setting lowered (`COMMONKIT_STORE_MIN_FREE_GB=1`) to check the rest of the path, and all six passed. **This needs fixing at source (commonkit), or the shells must declare a store policy:** see Surprises.
- **Swift, first build.** Swift 6 refused to send `Any` (a parsed reply) across the continuation. Fixed in the wrapper with a `Reply` box marked `@unchecked Sendable`, justified because the parsed objects are handed over, not shared.
- **iOS simulator, first build.** `@testable import` cannot work in a Release `xcodebuild`. The tests only use public API, so they now use a plain import.
- **`graph.get` measurement, first run.** The first poll after building the scene can hold only events (no reply yet). Fixed in the measurement loop, which now waits for the poll that carries the reply.

## What I did not verify

- **No physical device:** no iOS device run (the iOS device slice is built and in the xcframework, but nothing ran on it) and no Android phone or tablet.
- **JNI on Android:** the `.so` was cross-built and its exports checked. On the emulator only the Rust C-ABI tests ran; the JNI layer was tested on the host JVM only. No APK or instrumented test was built.
- **The x86_64 slices:** the x86_64 macOS slice is built into the universal library but was not run (no Rosetta run). `x86_64-linux-android` was not built.
- **The full workspace suite:** I did not run `cargo test --all-features` across the workspace (it drives the installed desktop apps and is slow), nor the Python suite. I ran the nearby crates above.
- **Real shell conditions:** no wake under a real UI's main-thread load, no memory-warning behaviour, no app sandbox, no hardened runtime or notarization.
- **The plan's hot path** (`plum_wire_paths`, hit testing) and the scene format: not in N0(b).
- **Emulator figures** are an emulator's on an M4 Max host, not a phone's.

## Surprises

1. **Two cores in one process work.** Each has its own session, engine and events; their graphs and runs don't mix, and closing one leaves the other running. Process-wide state seen so far (section 4.3's open question):
   - the ABI's own registries (cores, leases), by design;
   - commonkit's one hash algorithm per process (D8), which is shared but consistent;
   - the static command table, which is read-only;
   - the in-process engine host's session id, `format!("s-{}", process::id())`: **two in-process engines report the same session id.** Harmless here, but anything that keys on it (memo scopes, HTTP) would see one session where there are two. Worth checking in N1's two-sessions audit.
   - Two cores share one store when they share a `store_root`, which is fine because it is content-addressed.
2. **commonkit's mobile defaults decline everything.** `min_free_bytes` defaults to 8 GiB on every platform, so a phone or tablet with less than 8 GB free declines every blob. The emulator showed this, and iOS has the same default. `default_root()` is also `/dev/shm/commonkit` on iOS and Android, which the iPad spike already met.
   - **This needs the maintainer's hands:** fix it in commonkit (golden rule 6) with platform defaults for iOS and Android, or have the shells pass a policy through `Dirs.store`.
3. **Below 128 MB, the store reads and hashes rather than maps.** `Store::read_key` copies the segment into a `Vec` and verifies it; only arrays over `MAX_READ` are mapped. Section 7's "a blob from the store is often a mapped segment already" holds only for those large arrays.
   - The new `Store::map_handle` maps a plain segment by handle. It skips verification, as the store already does for large arrays.
   - Compressible values (`plumz`) can never be mapped, so on Apple the "no copy from the store" budget holds only for values stored plain.
   - Engines that publish pictures as arrays (`plumarr`) are never compressed, so pictures should be fine. Text and tables are not.
4. **A shared `target/` mixes deployment targets.** One build without `MACOSX_DEPLOYMENT_TARGET` left `blake3`'s C objects (built through `cc`) marked for macOS 27, and the Swift link then warned on every object. The two scripts now export the same value, and `cargo clean -p blake3 --release --target aarch64-apple-darwin` cleared it.
5. **Swift 6 has traps beyond the wake.** `Data`'s custom deallocator runs on whatever thread frees the `Data`, so it must be built in a nonisolated function. Built inside a `@MainActor` method, it would be inferred main-actor-isolated and trap like the wake. The wrapper builds both of its deallocators (poll buffers, blobs) in nonisolated functions.
6. **Sizes.**
   - Static archives: the universal macOS library is 129 MB, iOS 64 MB and the simulator 63 MB. The xcframework is 256 MB on disk.
   - The host `.dylib` is 13.8 MB and the Android `.so` 15 MB, release with debug info stripped. All of them hold the session and the text engine.
7. **`plum_poll` buffers are handed over without a copy on Apple** (`Data(bytesNoCopy:)` with `plum_free` as the deallocator). On JNI they are copied once into a `byte[]`, the one copy JNI needs.

## For the maintainer

- **The session's per-edit cost (`stale_reasons`, O(nodes × wires))** puts a 2,000-node edit at 30 ms, about twice the 16 ms budget. It is fixed by indexing wires by target once. This is a session change I did not make.
- **commonkit's `min_free_bytes` and `default_root` on iOS and Android:** fix at source, or decide that the shells always pass a policy.
- **Whether `plum_blob` may stay synchronous.** I recommend the core resolve blobs (a `blob.fetch` request answered after the wake), so `plum_blob` only hands out bytes already resolved and never stalls the UI thread.
- **Before committing:** the copied Gradle wrapper (`native/android/gradle/wrapper/gradle-wrapper.jar`, a binary from the native virtimp.com app), and whether `native/android/jniLibs/` and the xcframework stay uncommitted (both are in `.gitignore` files inside `native/`).
