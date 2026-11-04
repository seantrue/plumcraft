---
title: "npexact and numpy: the gap"
date: 2026-11-03
---

# npexact and numpy: the gap

The maintainer asked: "assess the gap between npexact and all of numpy".

## Summary

**npexact is not a numpy. It is a set of about 45 bit-exact scalar and slice routines, and numpy is around 460 top-level callables (106 ufuncs) plus about 1,000 more in its submodules.** By function count npexact covers about 3 to 5 percent of numpy. By what it *is*, the gap is larger: npexact has no array type, no dtypes beyond `f32` and `f64` (and `i64` outputs), no shapes, axes, strides or broadcasting. Each function takes a contiguous slice and reproduces one numpy code path to the bit.

That is what it was built for, and it does that well: every function cites the numpy 2.5.3 source it mirrors and is held to fixtures numpy itself recorded, compared on bit patterns with no tolerance. "All of numpy" is the wrong target. The useful targets are:

1. **What PlumCraft and the research software actually call.** About 20 numpy behaviours are already hand-ported inside `plumcraft-array` rather than in npexact (section "Already ported, in the wrong place"). Moving them upstream is the cheapest, most valuable step.
2. **The `Generator` distributions.** 11 of 43 draws are there; the gamma family underpins most of the rest.
3. **Cross-host exactness.** Every fixture was recorded on one machine (darwin-arm64, M4 Max). numpy's SIMD paths differ by CPU (NEON, AVX2, AVX-512), so bit-exactness on the Linux x86-64 server is unproven.

## What npexact is

Source: `yapocis-rs/crates/npexact`, 0.1.0 on crates.io, no dependencies. About 4,300 lines, 2,700 of them the ziggurat tables.

| Module | numpy equivalent | Public functions |
|---|---|---|
| `random` | `SeedSequence`, `PCG64`, `Generator` | `SeedSequence`, `Pcg64`, `Generator::{default_rng, random_f64/f32, uniform, integers, integer, integers_broadcast, standard_normal, normal, standard_exponential, exponential, geometric, shuffle, permutation, choice_without_replacement}`, `Contraction` (FMA or not, as the wheel was built) |
| `reduce` | `sum`, `mean`, `var`, `std`, `add.reduceat` | f32 and f64 each; pairwise summation in numpy's order |
| `hist` | `linspace`, `histogram`, `cumsum`, `interp` | `linspace_f32/f64`, `histogram_f32`, `histogram_range_f32`, `histogram_density_f32`, `cumsum_f64`, `interp` |
| `order` | `partition`, `quantile` (linear), `median` | f32 only |
| `simd_math` | `sin`, `cos`, `tanh`, `abs(complex64)` | numpy's own SIMD polynomials, not libm |
| `fsum` | Python's `math.fsum` | one function |

The crate description says "sorts", but there is no sort: `order` has partition, quantile and median only.

## The gap, area by area

Counts are from numpy 2.5.4 (the version in `python/`'s environment); npexact was ported from 2.5.3.

| numpy area | Size in numpy | In npexact | Gap |
|---|---|---|---|
| The `ndarray` itself: dtypes, shapes, strides, views, broadcasting, indexing, casting rules | the core of numpy | none | Total. npexact works on `&[f32]`/`&[f64]`; callers use `ndarray` or their own buffers |
| Dtypes | bool, 10 integer, float16/32/64/longdouble, complex64/128, datetime, timedelta, string, object, structured | f32, f64 (i64 out) | All integer arithmetic, float16, complex (except `abs` of complex64), datetime, strings |
| Ufuncs | 106 (arithmetic, comparison, bitwise, 30+ transcendentals, rounding, floating-point helpers) | `sin`, `cos`, `tanh` (f32; tanh also f64), `abs` of complex64 | About 100. The ones that matter for exactness are the SIMD-dispatched ones: `exp`, `log`, `exp2`, `log2`, `sqrt` (IEEE, safe), `arctan2`, `power`, `hypot`, f64 `sin`/`cos`. Plain `+ - * /`, `minimum`, `maximum`, `floor`, `rint` are IEEE and need only NaN and signed-zero care |
| Reductions | `sum`, `prod`, `mean`, `var`, `std`, `min`, `max`, `argmin`, `argmax`, `cumsum`, `cumprod`, `nan*` variants, `ptp`, `average`, all with `axis`, `keepdims`, `dtype`, `where` | `sum`, `mean`, `var`, `std` (whole array, contiguous), `cumsum` (f64), `reduceat` add | Axes (a reduction along a non-contiguous axis is not pairwise in numpy and has its own order), `prod`, `cumprod`, `nan*`, `average`, `ddof` |
| Sorting and searching | `sort` (quicksort/introsort, stable radix/timsort), `argsort`, `lexsort`, `partition`, `argpartition`, `searchsorted`, `unique`, `where`, `nonzero`, set operations | `partition` for one `k` (by Rust's select, which gives numpy's value but not its arrangement) | Everything else. Values of a sort are unique, so only `argsort` ties, `unique`'s inverse and `partition`'s arrangement need numpy's algorithm |
| Statistics | `quantile`/`percentile` (13 methods), `median`, `histogram` (bin rules, weights, non-uniform edges), `histogram2d`, `histogramdd`, `bincount`, `digitize`, `corrcoef`, `cov`, `correlate`, `convolve` | linear quantile and median (f32), uniform-bin histograms (f32) | Other quantile methods, `nan` variants, weighted and 2-D histograms, `bincount`, `convolve`, `cov` |
| Creation and manipulation | `arange`, `linspace`, `logspace`, `geomspace`, `eye`, `meshgrid`, `reshape`, `concatenate`, `stack`, `pad`, `tile`, `roll`, `flip`, `transpose`, … (about 120) | `linspace` | Mostly not a numerics question: `ndarray` covers the layout. `arange` with float steps, `logspace` and `geomspace` do need numpy's arithmetic |
| `random.Generator` | 43 draws and helpers; 5 bit generators; `SeedSequence.spawn` | 11 draws, `PCG64`, `SeedSequence` | 32 draws: `standard_gamma` first (it underpins `gamma`, `beta`, `chisquare`, `dirichlet`, `f`, `standard_t`, `negative_binomial`), then `poisson`, `binomial`, `lognormal`, `laplace`, `logistic`, `gumbel`, `weibull`, `pareto`, `rayleigh`, `triangular`, `vonmises`, `multivariate_normal` (needs `linalg`), `choice` with replacement or `p`, `permuted`, `bytes`. Bit generators `MT19937`, `PCG64DXSM`, `Philox`, `SFC64`; `spawn`; `jumped` |
| Legacy `random` (`np.random.seed`, `RandomState`, 49 members) | MT19937 with the legacy (polar) normal and different integer bounding | none | All of it. Any research code still on `np.random.seed`/`np.random.rand` can't be reproduced |
| `linalg` | 33 (LAPACK/BLAS: `solve`, `inv`, `det`, `eig`, `svd`, `qr`, `cholesky`, `lstsq`, `norm`, `matmul`) | none | All. Bit-exactness here depends on the BLAS build (OpenBLAS vs Accelerate, thread count), so it is not reachable in general. `matmul` of small f32 matrices and `norm` could be done for one build |
| `fft` | 19 (pocketfft C++) | none | All. `plumcraft-array` has its own FFT (`generate/fft.rs`), held to 1e-6, not bits |
| `polynomial` | 8 classes plus helpers | none | All; `polyfit` needs `lstsq` |
| `ma`, `strings`/`char`, `datetime`, I/O (`load`, `save`, `loadtxt`), `testing` | about 370 | none | Out of scope for a numerics crate. `.npy` I/O exists in Rust (`ndarray-npy`, already a dev-dependency) |

## Already ported, in the wrong place

`plumcraft-array` reproduces these numpy behaviours itself, each written and checked against numpy by hand. They are npexact's job, and the research software's Rust ports probably need some of them too.

| numpy behaviour | Where in PlumCraft |
|---|---|
| `searchsorted(side="left")` with NaN last | `crates/array/src/math/kernels.rs` |
| `nanpercentile` (linear) | `math/kernels.rs` |
| `nanmin`, `nanmax`, `min`/`max` with NaN propagating | `math/kernels.rs`, `filters/kernels.rs` |
| `nan_to_num` | `math/kernels.rs` |
| `mod` (float32 `np.mod`, divisor's sign, signed zero) | `math/kernels.rs` |
| `minimum`, `maximum` (NaN wins), `sign`, `clip(v, 0, None)` | `math/kernels.rs`, `filters/kernels.rs`, `colour/mod.rs` |
| `cumsum` of int64 | `math/kernels.rs` |
| `histogram` with explicit float32 edges, `histeq` density | `math/kernels.rs` (partly over npexact) |
| `linspace(dtype=float32)` | `generate/noise.rs`, a duplicate of `npexact::hist::linspace_f32` |
| complex64 multiply with numpy's FMA contraction | `generate/noise.rs` |
| `fft.fft`, `ifft`, `fftshift`, `ifftshift` | `generate/fft.rs` (1e-6, not exact) |
| `unique` with its inverse, `rint` (half to even), `sort` NaN last | `generate/regions.rs` |
| `lexsort` (stable, by keys) | `filters/trace.rs` |
| NumPy 2 weak Python scalars in float32 comparisons and arithmetic | `filters/mod.rs`, `filters/kernels.rs`, `filters/trace.rs` |

## Quality notes

- **Exactness is proven on one host.** All 70-odd fixture sets are under `fixtures/host/darwin-arm64-apple-m4-max`. numpy picks SIMD kernels at run time; `sin`, `cos`, `tanh` and the `abs` of complex64 have per-ISA variants, and `Contraction` already models one of the differences (FMA). The x86-64 Linux build PlumCraft now ships should get its own fixture set before anyone relies on bit-exactness there.
- **Version drift.** Ported from numpy 2.5.3; `python/` has 2.5.4. A fixture rerun against each numpy release would catch a changed kernel.
- **Panics.** npexact is "no `unwrap` or `panic`" by its evaluation, but non-test code has five `assert!`s: `histogram_f32` and `histogram_range_f32` (`bins >= 1`), `interp` (lengths), `quantile_linear_f32` and `median_f32` (empty input). numpy raises in each case. Under PlumCraft's golden rule 1 they should return `Option` or `Result`; PlumCraft's callers check first today.
- **Shape of the API.** Everything is a free function on contiguous slices with a type suffix. Adding axes or strides later means a second API, not an extension; worth deciding before the crate grows.

## Recommendation

Not "all of numpy". In order:

1. **Move PlumCraft's ~20 hand-ported behaviours into npexact** with recorded fixtures, and make `plumcraft-array` call them. Drop the duplicate `linspace`. Small, and it makes the crate the one place numpy's numerics live.
2. **Replace the five `assert!`s** with `Option`/`Result` (a 0.2.0, as it changes signatures).
3. **Record an x86-64 Linux fixture set** (AVX2, and AVX-512 if the server has it) and run the tests there.
4. **Grow by demand, not by catalogue.** The likely next needs, given image work: `exp`, `log`, `power`, `arctan2`, f64 `sin`/`cos` (SIMD ufuncs whose results differ from libm); `argsort` with numpy's tie order; reductions along an axis; `standard_gamma` and `poisson`; `fft` to the bit if a graph's results must match the research software's exactly.
5. **Leave out** `linalg` (BLAS-dependent, so bits aren't reproducible across builds anyway), `ma`, strings, datetime and I/O. Legacy `RandomState` only if research code turns out to use `np.random.seed`.

**Not checked:** which numpy calls the research software's Python makes that its Rust ports need; that survey would turn step 4 from a guess into a list.
