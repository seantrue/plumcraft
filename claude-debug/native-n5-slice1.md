# N5 slice 1: the Windows kit

2026-11-21, branch `native/n5-kit` from `feat/native-n0` at `5f0cb5f` (not committed). The record is in `docs/research/24-native-shells.md` section 21; this report holds the counts, the timings, how they were taken, and what was found.

## Answer

The spike's `PlumCraft.Core` is now the Windows kit, five projects under `native/windows/` that mirror the Swift kit: models for the contract, the core wrapper with a parsed feed, an observable document model applying deltas by nid, the canvas link, and the conformance runner with a driver through the kit and one over a control port. On the Windows 11 ARM64 VM, `dotnet test` passes 34 of 34 tests, and the whole conformance suite passes through the kit headless, 51 of 51, the same count as the C ABI's own Rust headless driver (which also passes there). The probe and the WinUI spike still build against the changed kit and the probe passes. No Rust changed.

## What was built

| Project | What it holds |
|---|---|
| `src/PlumCraft.Models` | The contract's types as hand-written records with `System.Text.Json` source generation; words kept as strings beside a typed reading; `Shell.Options` (skips unknown fields) and `Shell.Strict` (refuses them, for tests); `Jsonv` (JSON values as `JsonElement`, pointers, the suite's equality) |
| `src/PlumCraft.Core` | The spike's P/Invoke layer and pump, now with a parsed feed fanned out to subscribers (`Subscribe`), typed requests decoded on the thread pool, refusals as `PlumException` with the core's words and `code`, the hot calls' results read in words (`HotCalls.cs`) |
| `src/PlumCraft.Kit` | `DocumentModel` (one document on one `SynchronizationContext`, items by nid, an `ItemModel` per item told only when its `rev` changes), `CanvasLink` (geometry typed and JSON, parts, hits, wire targets, gestures, typed drag moves and wire paths, nudges), `SerialContext` (the headless stand-in for a UI thread) |
| `src/PlumCraft.Conformance` | The runner of contract 16.1 in C#, `KitDriver` and `PortDriver` |
| `tests/PlumCraft.Kit.Tests` | xUnit, 34 tests |

The kit never implements a user action: it dispatches core commands by id (`RunAsync`, `InvokeAsync`, `ActivateAsync` for a menu item) and sends recognised input as `ui.gesture`. The only `unsafe` code is the interop layer: `Native.cs` (the declarations) and `PlumCore.cs`'s pointer methods (open, request, post, blob, wire paths, geometry).

## The machine and how it was run

| What | Value |
|---|---|
| Machine | The Windows 11 Pro ARM64 VM (UTM, 4 cores, 8 GB, WARP), .NET SDK 10.0.404, Rust 1.99.1 |
| Checkout | `%USERPROFILE%\work\plumcraft` at `9b90500` with `core.symlinks true`; this branch's `native/windows` copied over it (`k1-push.sh`). `9b90500` and `5f0cb5f` differ in no Rust, fixture or schema file, so the DLL the spike built there is this branch's core |
| Core | `plumcraft_ffi.dll` from the spike's release build in the checkout's `target` (not rebuilt) |
| Tests | `k1-test.ps1`: `dotnet test tests\PlumCraft.Kit.Tests -c Release`, with `TEMP`, `TMP`, `PLUMCRAFT_TEST_SCRATCH`, the store, marks, test-run, craft and preferences roots all under `%USERPROFILE%\work\n5-scratch\k1-tests`; each core also gets its `dirs` there |
| Mac | Nothing built or run on the Mac but `pandoc`; no Rust, Swift or .NET there |

## Results

| Run | Tests | Result | Time |
|---|---|---|---|
| First | 33 | 31 passed, 2 failed (both found defects, below) | 23 s with the build |
| Second, after the fixes (one test added) | 34 | 34 passed | 25 s with the build, 17.5 s of tests |
| Third, fourth, fifth | 34 | 34 passed each | 15 s each, 11.8 to 12.3 s of tests |

| Conformance, through | Cases | Result | Time |
|---|---|---|---|
| The kit, a core on the text engine, every case | 51 | 46 passed, 5 skipped (the array engine's), 0 failed, 623 steps | 2.3 to 2.4 s |
| The kit, a core on the array engine, the array cases | 5 | 5 passed | 0.2 to 0.3 s |
| **The kit, both (the gate)** | **51** | **51 passed** | **2.5 to 2.7 s** |
| The kit, the read half alone | 30 | 25 passed, 5 skipped (the array engine's), 313 steps | 0.1 to 0.2 s |
| A core's control port (`PortDriver`, text engine) | 51 | 46 passed, 5 skipped (the array engine's) | 5 to 9 s |
| The C ABI's Rust driver (`cargo test -p plumcraft-ffi --release --test conformance`, on the VM) | 4 tests | 4 passed (the whole suite through the C ABI with the array cases on an array core, the read half, the array cases, the control port) | 8.7 s |

The number to match: note 24 section 19 (N4's exit) has 51 cases passing on the C ABI headless; its Rust test runs every case on a text-engine core but the array engine's five, which it requires to be the only ones skipped, and those five on an array-engine core. The C# gate test asserts the same: 46 and 5 skipped, then 5 of 5, 51 passed in all, no case failed or left out.

Measured at 500 nodes and 800 wires (`Measure` test, medians, five runs):

| What | Value |
|---|---|
| A whole scene, `scene.get` request to reply | 30.5 to 80.9 ms (1,269 KB) |
| Decoded into the C# models | 15.3 to 45.9 ms (one run 110.7 ms) |
| Applied to the document model | 0.22 to 0.41 ms |
| A one-node `node.set` to its face's new revision in the model | 5.0 to 11.0 ms p50, at most 22.2 ms |

Other numbers from the tests: `command.list` 164 commands; the probe's run gave 15 `node.status` events by the feed in 23 wakes and 23 polls; 500 requests in flight came back to their own callers in 494 polls; a 64 by 48 picture from the array engine was lent as 12,288 bytes. The spike's probe, run again against the changed wrapper: ping 22 us, `graph.get` 2.8 ms at 500 nodes and 17 ms at 2,000, `scene.get` 36 and 154 ms, `plum_hit_test` 0.62 and 2.54 us, a 64 MiB blob borrowed in 49 us; PROBE PASSED.

## Found

| What | Where | Done |
|---|---|---|
| The schema's (`shell1.json`) `Gesture` has no `zoom`, which the core takes since N4 slice 2 (contract 8.13, `gesture.rs`): the schema's type (`shell_schema.rs`) lacks the field, so the schema is stricter than the core | The kit's test that what the shell sends is in the schema | Reported, not fixed: it is a Rust change and a regenerated schema, which this slice does not build. The test sends a drag without `zoom` and says why |
| The source generator sets a missing init-only property to null, so a field the kit defaults (a menu item's `items`, `look`, `id`, `label`; a shortcut's `mods`; a scene's or delta's `view`) lost its default: a menu without `items` crashed `Flatten()` | The first test run (`ThemeMenusAndAContextMenu`) | Fixed: those fields read through a backing field; a test decodes `{}` as a menu item and a scene without `view` |
| `PlumCore.WirePaths` could return a count larger than the buffer when the snapshot grew between its two calls, and `DecodePaths` would then index past the buffer | Reading the spike's code | Fixed: up to four tries with room, never a count past the buffer; `DecodePaths` bounded by the buffer |

The core behaved on Windows as on the Mac: no case failed, and no contract ambiguity met beyond the schema's `zoom`. The one refusal `code` the tests met was `unknown_doc`, on a request for a document that isn't open, as contract 6.1 says.

## Defects introduced and caught

| Defect | Caught by | Fixed |
|---|---|---|
| An XML comment with a double hyphen in `Directory.Build.props` | The first build | Reworded |
| Two calls with one parenthesis too many | The build | Fixed |
| `Half` clashed with `System.Half` in the tests | The build | The enum is `CaseHalf` |
| A possibly null lease dereferenced in a test | The build (nullable warnings as errors) | Fixed |
| Defaulted init-only fields set to null (above) | The first test run | Fixed |
| The schema test sent `zoom`, which the schema lacks (above) | The first test run | The test sends none; the gap reported |
| `Hit` and `PlumHit` readings named like their enum types (`HitKind HitKind`) | Review before the first build | Renamed `KindOf`, `PartOf` |
| A `Windows` record that would meet WinUI's `Windows` namespace | Review | `DocumentWindows` |

## Decisions this slice leaned on

| Decision (note 24, 20.8) | What the slice did |
|---|---|
| 5, the C# models | Hand-written with `System.Text.Json` source generation (recommended) |
| 4, packaging | Unpackaged: the kit is plain `net10.0` libraries and the tests run from `dotnet test`; the spike stays unpackaged and self-contained (recommended) |
| 1, symlinks | The VM's checkout keeps `core.symlinks true`; the repository's symlinks untouched (recommended) |
| 6, supported Windows | Not touched: the kit targets `net10.0`, nothing Windows-only below the window |
| 7, CI | None added |

Choices of this slice's own, for the maintainer to see: xUnit 2 (not MSTest or xUnit 3) as the test framework; words as strings with a typed reading beside them (not a generic open-enum converter); the document model tells an item's view only when its `rev` changes (Swift also compares the item's content when the revision is the same; the contract makes the revision enough, 8.12); the model needs a `SynchronizationContext` and refuses to start without one; `DocumentWindows` for Swift's `Windows`, `CaseHalf` for Swift's `Half`.

## On the VM

| What | Value |
|---|---|
| Free space before | 12.13 GB |
| Free space after | 11.88 GB |
| NuGet packages added | Microsoft.NET.Test.Sdk 17.14.1, xunit 2.9.3, xunit.runner.visualstudio 3.1.0, and what they bring (Microsoft.CodeCoverage, Microsoft.TestPlatform.ObjectModel and TestHost, Newtonsoft.Json, the xunit parts): 70.4 MB in the NuGet cache |
| Left there | `native\windows` (this branch's copy, with build outputs, 357 MB), the scratch files `%USERPROFILE%\work\n5-scratch\k1-*` (logs, trx files, `k1-tests`, `k1-rust`, `k1-probe-root`, under 10 MB), the Rust conformance test binary in the existing target folder |
| Not left | No scheduled task was made; no process of this slice is running (checked) |

## Not done

- The window on the kit: the spike's window still uses its own scene model and the raw feed (slice 2 replaces it).
- A command-line runner in C# (Swift has `plumcraft-conformance`): the Swift runner already drives a Windows app over a port, and slice 2's gate can use either.
- Models for settings (`settings.get`), reports, compare and the viewers' data: the Swift kit has them for N4's later slices; they come with slice 4.
- The schema's `zoom` (above).

## Scripts and logs

In `claude-debug/native-n5/` (not tracked): `k1-push.sh` (copy `native/windows` to the VM), `k1-build.ps1` (build one project), `k1-test.ps1` (the tests, every root in scratch), `k1-rust-conformance.ps1` (the C ABI's Rust driver on the VM), and the logs `k1-first.log`, `k1-second.log`, `k1-run3.log` to `k1-run5.log`, `k1-first-cases.txt` (every case's line). The spike's `win.sh` and `pull.sh` in the main checkout's `claude-debug/native-n5/` were used as they are.
