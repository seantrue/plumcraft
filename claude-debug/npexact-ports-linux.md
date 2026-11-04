---
title: "npexact: the array engine's numpy behaviours, and Linux fixtures"
date: 2026-11-03
---

# npexact: the array engine's numpy behaviours, and Linux fixtures

The maintainer asked: "Move the hand-ported behaviours into npexact, record fixtures on Linux and test." This follows `npexact-numpy-gap.md`.

## Summary

- **Moved.** The 15 families of numpy behaviour that `plumcraft-array` had written itself now live in npexact, each held to fixtures numpy recorded. `plumcraft-array` calls them through its own small wrappers, so its nodes and tests didn't change.
- **Recorded on Linux.** yellowfin (Xeon E5-2698 v4, x86-64, AVX2, no AVX-512) now has a full fixture set beside the M4 Max's, from the same numpy 2.3.3 and Python 3.13.7.
- **Tested.** All npexact tests pass on both machines, and the pure-Rust sections replay both machines' fixtures on either. All of `plumcraft-array`'s tests pass on the Mac. On yellowfin one fails, the same way it does without this work.
- **The fixtures found four things:**
  - two bugs in npexact's existing code: `quantile` on both hosts, and `sin`/`cos` on x86-64;
  - one in PlumCraft's hand port: `clip(-0, 0)` gave `-0`;
  - one difference between the two CPUs: how numpy's `maximum`/`minimum` break a tie between `-0` and `+0`.

Nothing is committed, pushed or published.

## What moved

| numpy | npexact | Was in `plumcraft-array` |
|---|---|---|
| `minimum`, `maximum` | `elementwise::{minimum_f32, maximum_f32}` (+ `_with`) | `math/kernels.rs` (Arithmetic) |
| `clip(v, 0, None)` | `elementwise::clip_min_f32` | `colour/mod.rs` |
| `sign` | `elementwise::sign_f32` | `filters/kernels.rs` (Zero Crossings) |
| `mod` | `elementwise::mod_f32` | `math/kernels.rs` (Wrap Angle) |
| `nan_to_num` | `elementwise::nan_to_num_f32` | `math/kernels.rs` |
| `min`, `max` | `extrema::{min_f32, max_f32}` | `math/kernels.rs`, `filters/kernels.rs` |
| `nanmin`, `nanmax` | `extrema::{nanmin_f32, nanmax_f32}` | `math/kernels.rs` |
| `cumsum` (int64) | `extrema::cumsum_i64` | `math/kernels.rs` |
| `searchsorted` | `order::{searchsorted_left_f64, searchsorted_right_f64}` | `math/kernels.rs` (Histogram Match) |
| `lexsort` | `order::lexsort_f32` | `filters/trace.rs` |
| `nanpercentile` (linear) | `order::nanpercentile_linear_f32` | `math/kernels.rs` (Stretch, curves) |
| `unique(return_inverse=True)` | `order::unique_inverse_i64` | `generate/regions.rs` |
| `sort` | `order::sort_f32` | `generate/regions.rs` (Segment Average) |
| `linspace(dtype=float32)`, Python-float bounds | `hist::linspace_f64_as_f32` | `generate/noise.rs` |
| `fft.fftshift`, `fft.ifftshift` | `fft::{fftshift_2d, ifftshift_2d}` | `generate/fft.rs` |

**Left in PlumCraft, on purpose:**

- **The FFT transform:** it is held to 1e-6, not bit for bit, and npexact only holds exact ports. Moving it there would need pocketfft itself ported.
- **Mandelbrot's complex arithmetic and the elementwise kernels:** these copy numba, not numpy.
- **`rint`:** Rust's `round_ties_even` is already exact.
- **NumPy 2's weak-scalar rules:** a rule for callers to follow, not a function.

The existing `quantile_linear_f32` and `median_f32` had no fixtures. They have them now, and that is how the first bug below was found.

## Fixtures

The recorder is Yapocis3's `src/yapocis/codegen/host_fixtures.py` (branch `host-fixtures-ports`). It gains four sections (`elementwise`, `extrema`, `order`, `fft`) plus three `linspace` cases in `hist`. Its inputs include:

- every pair of 25 special values;
- long arrays with NaN and signed zeros sprinkled in;
- 75 lengths for the reductions, so the SIMD body and tail are both exercised;
- all-NaN arrays and ties.

The script needs only numpy. On yellowfin it ran from a temporary directory (`uv run --no-project --with numpy==2.3.3`), with yellowfin's own checkouts untouched.

Re-recording the Mac gave byte-identical files for every earlier case. Only `hist.npz` changed, since it gained three keys.

| Host | Check | numpy results |
|---|---|---|
| darwin-arm64 (M4 Max) | `fma` (`normal`, `uniform`) | fused |
| darwin-arm64 (M4 Max) | `exp_f32`, `log_f32` | libm |
| x86-64 (Xeon E5-2698 v4) | `fma` (`normal`, `uniform`) | plain |
| x86-64 (Xeon E5-2698 v4) | `exp_f32`, `log_f32` | numpy's own AVX2 |

## What the fixtures found

1. **npexact `quantile_linear_f32`, wrong on both hosts.** With a Python-float `q`, NumPy 2 turns `q` into float32 and does everything after it in float32: the virtual index `(n - 1) * q`, the floor, gamma and the interpolation. npexact did the first steps in float64. It was 1 ulp off, for example at `q = 0.9` with 2 or 5 values. Fixed; every recorded quantile now matches.
2. **The signed zeros in `maximum` and `minimum` depend on the CPU.**
   - On arm64, numpy's loops use `fmax`/`fmin`, which follow IEEE: `+0` is the larger zero, so the order doesn't matter.
   - On x86-64 they use `maxps`/`minps`: `a > b ? a : b`, so a tie gives the second operand.
   - As a result, x86's `max`, `min`, `nanmax` and `nanmin` of an array depend on numpy's AVX2 reduction order. npexact now reproduces that order step by step from numpy 2.3.3's `loops_minmax.dispatch.c.src` and `simd/avx2/math.h`. The tail uses `maxss` for `max`/`min` (numpy replaces its scalar op on x86) and glibc's `fmaxf` for `nanmax`/`nanmin`.
   - This is `elementwise::MinMax` (`Ieee`, `X86`). All 2,054 `extrema` checks and 24 `elementwise` checks match on both hosts. `claude-debug/npexact/avx2_reduce_sim.py` is the Python model used to find it.
3. **PlumCraft's `clip(v, 0, None)` returned `-0` for `-0`.** numpy clips through `maximum`, which gives `+0` on both CPUs. Fixed by the move.
4. **npexact `sin_f32`/`cos_f32` on x86-64: 470 of 355,342 lanes off by 1 ulp**, all near odd multiples of π/4.
   - numpy's source rounds the quadrant with a multiply, then `+ magic - magic`.
   - GCC, which builds the manylinux wheels, fuses `x * 2/π + magic` into one FMA. Clang, which builds the macOS wheels, doesn't.
   - This is `simd_math::Quadrant` (`Separate`, `Fused`). The fused model matches every x86 lane, and the separate one every arm64 lane. `claude-debug/npexact/sincos_x86_probe.py` and `sincos_x86_diff.py` found it.
5. **`np.sort` doesn't fix where `-0` and `+0` land.** They compare equal, and numpy's sort algorithm differs by CPU (Highway on arm64, x86-simd-sort on x86). `sort_f32` matches numpy's values; its zeros' signs are documented as not reproduced, and the test ignores them.

The `simd_math` test also assumed numpy's float32 `exp`/`log` are libm, which is true only on arm64. npexact ports neither, so its expectation now follows the architecture.

## Test results

| Where | What | Result |
|---|---|---|
| Mac | `cargo test -p npexact --release` | all pass (7 suites) |
| Mac | `cargo clippy --workspace --all-targets` (yapocis-rs, `ocl`, `metal`) with `-D warnings`, `cargo fmt --check` | clean |
| yellowfin | `cargo test -p npexact --release` against its own fixtures, plus clippy | all pass, clean |
| both | `elementwise`, `extrema`, `order`, `fft` replayed for **both** hosts | 24, 2,054, 84 and 7 shapes per host |
| Mac | `cargo test -p plumcraft-array --release` | 52 passed |
| Mac | PlumCraft gate: `cargo test --all-features`, clippy with `-D warnings`, fmt; `pytest`, `ruff` | 1,037 passed, 0 failed, 3 ignored; clean; 208 passed; clean |
| yellowfin | `cargo test -p plumcraft-array --release` | 51 passed, 1 failed: `mandelbrot_matches_numba`, 9 points. The unmodified code fails identically: the references are the Mac's numba (fused complex multiply) and x86 uses the plain contraction. Not caused by this work |

## Still to do

- **Publish npexact.** The changes are additive (new modules, `_with` variants, two policies) plus two bug fixes, so 0.1.1 fits. npexact takes its version from the yapocis-rs workspace (0.1.0), so it needs its own `version` or a workspace bump. Then remove `[patch.crates-io]` from PlumCraft's `Cargo.toml`. Publishing `plumcraft-array` needs this first.
- **AVX-512.** A CPU with it runs numpy's AVX-512 loops, with 16 lanes and its own reductions. Neither the zero-sign rule nor the quadrant rounding is proven there.
- **The array engine's other hosts.** Its references are the Mac's; Mandelbrot shows that an x86 run can't match them all.
- **Docs and correction.** `lib.rs` says npexact was ported from numpy 2.5.3's sources, while every fixture was recorded with 2.3.3. `npexact-numpy-gap.md` repeated "2.5.3". (Its remark that npexact has no sort is now out of date.)
