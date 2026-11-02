---
title: "array-rs: an array engine on yapocis-rs"
date: 2026-10-29
---

# array-rs: an array engine on yapocis-rs

The maintainer asked: "yapocis-rs already has operations for all of this, I think. Evaluate an 'array-rs' implementation that is a stripped down version of yapocis-rs? (nothing from yapocis-external)"

## Summary

**Recommendation: build it, but as a dependency, not a copy.** Add a PlumCraft engine crate, `plumcraft-array`, that depends on yapocis-rs at a pinned git revision with only the features it needs. A stripped copy inside PlumCraft would break two golden rules and would drift from the kernels' source of truth.

yapocis-rs covers about 37 of the 52 general array nodes in the node survey directly, with the same GPU kernels the research software supporting www.virtimp.com runs. About 13 more are small CPU functions that today come from yapocis_external. Two (Mean Shift and Posterize Hue) have no reasonable replacement and are dropped for now.

**Checked here:** `cargo build -p yapocis --features metal --release` takes 13 s. The `processing` tests pass on this Mac's Metal device (8 of 8, matching the Python package's recorded outputs).

## What yapocis-rs is

| Crate | What it is | Size |
|---|---|---|
| `yapocis` | Typed kernel API generated from a vendored bundle, plus `lib::*` ports of the Python library | ~9k LOC; bundle 18 MB (kernel sources for three backends plus parity fixtures) |
| `yapocis-rt` | Devices, buffers, program cache, launches on OpenCL, Metal and CUDA | ~2.5k LOC; FFI `unsafe` in `ocl.rs`, `metal.rs`, `cuda.rs` |
| `yapocis-bundle` | Manifest schema and the codegen `build.rs` runs | no GPU dependencies |
| `npexact` | Bit-exact numpy host numerics (PCG64, reductions, transcendentals) | no `unwrap` or `panic` |

There is **no CPU backend**: every kernel needs a Metal, OpenCL or CUDA device. The Python package stays the source of truth for the kernels, and the Rust crate's parity fixtures hold it to them.

## Coverage of the 52 nodes

| Group | From yapocis-rs | Small CPU code (was yapocis_external) | Dropped |
|---|---|---|---|
| Arrays and channels (11) | Load and Save Image (`image` crate: PNG, JPEG), Split, Pick Channel, Stack (`lib::image`), Resize (`pil::resize`), Crop to Multiple (`align_image`), Frame (`set_frame`), Constant | Rotate (nearest neighbour), Convert Color Space for Lab and other spaces beyond HSI and colormath's matrices | — |
| Maths (12) | Arithmetic (`operators`), Clamp, Stretch, Binarize, Histogram, Equalize, Histogram Match (`imageops`), Normalize (`image`), Wrap Angle (`typeset::compute_360_to_180`) | Matrix Multiply, Set Where, Apply Curve (the research software's `core.curves`) | — |
| Filters (6) | Gaussian Blur, Convolve, Custom Convolve (`gaussian`, `convolve`), Median (`median`), Perona–Malik (`anisodiff`) | — | Mean Shift |
| Edges (5) | Gradient (`gradient`), Zero Crossings (`zcs`), Trace Contours (`truetrace`) | Sobel Magnitude, Segment Edges | — |
| Regions (4) | — | Connected Components (union-find), Region Outlines, Grid Segments, Segment Average | — |
| Generate (6) | Simplex Noise (`simplex`), Mandelbrot (`mandelbrot`), Random Points (`npexact` PCG64, numpy's own draws) | Coordinates, Game of Life, Mask Points | — |
| Draw (4) | Draw Shapes, Draw Lines, Composite (`graphics`), Draw Text (`typeset`, bundled FreeType) | — | — |
| Frequency (2) | FFT, Inverse FFT (`fft`) | — | — |
| Colour (2) | Color Matrix (`colormath`) | — | Posterize Hue |

## Why a dependency, not a stripped copy

| Concern | A copy inside PlumCraft | A pinned git dependency |
|---|---|---|
| Golden rule 2 (no `unsafe`) | Violated: the Metal, OpenCL and CUDA bindings are FFI | Fine: it is another crate's code, like `std` |
| Golden rule 1 (never panic) | Violated: about 200 `unwrap`, `expect` and `panic!` in non-test code across `yapocis`, `yapocis-rt` and `yapocis-bundle` | Contained: the engine runs as its own process (mode 2), and the host already restarts a crashed engine. The panics get fixed at source (rule 6), in yapocis-rs |
| Kernel drift | The bundle is regenerated from Python yapocis in yapocis-rs; a copy would fall behind | Moves with the pinned revision |
| Stripping | By hand, file by file | By features: `metal` on macOS, `cuda`/`ocl` on Linux; `text` (FreeType) only if Draw Text stays |

## Strengths

- **The research software's numbers.** Same kernels, held to the Python package by parity fixtures, so an array graph gives the same results in PlumCraft as in the research software on the same device. A numpy rewrite would only approximate them.
- **Rust end to end.** All four hosting modes work, no Python environment is needed, and arrays stay `Data::Array` in the Rust SDK.
- **Speed.** GPU kernels on full-resolution images; the program cache keeps compiled kernels.
- **Already written.** Most nodes are a thin wrapper: ports, a call, and conversion between `ndarray` and `Data::Array`.

## Risks and costs

1. **A GPU is required.** No CPU fallback: a machine without Metal, OpenCL or CUDA has no array engine, and GPU-less CI can only test the CPU nodes. the development machine (Metal) and a remote server (CUDA) both qualify.
2. **Panics in the dependency.** About 200 sites. Mode 2 contains them, but mode 1 (in-process) would rely on the host's panic catching. Fixing them in yapocis-rs is the right answer.
3. **Last-bit differences between devices.** yapocis-rs records parity per device. Persisted results already stay per machine (`machine` scope).
4. **Colour spaces.** Lab and the other spaces in the research software's `colorspaces` aren't in yapocis. They need a careful CPU port to match the research software.
5. **Build weight.** yapocis-rs uses ndarray 0.17, objc2 (macOS), opencl3 and cudarc, and FreeType (bundled C) with `text`. It builds in 13 s here.
6. **Dropped nodes.** Mean Shift and Posterize Hue are yapocis_external kernels with no simple CPU equivalent.

## Independent of the choice

Making arrays first class needs these whichever engine computes them:

- **Core type.** ARRAY joins the core types (golden rule 3 amended): a pin shape, a layout weight, and conversions where engines declare them.
- **Previews in the SDKs.** The engine SDKs produce what spec section 10 promises:
  - a node thumbnail for array outputs;
  - one rendition per channel or colour view;
  - per-channel stats and histograms;
  - the view modes the research software has (auto, color, stretch, clip, heat, labels, angle).

  Today an array gets only a summary and `full`, and the UI plots 1-D arrays only.
- **An array viewer in the editor.** The view-mode picker, a channel grid, stats and zoom, ported from the research software's `viewer.rs`.

## Proposed plan

1. **Arrays first class:** the core type, SDK previews (Rust and Python) and the editor's array viewer.
2. **`plumcraft-array`:** a Rust engine crate depending on yapocis-rs (pinned revision, `metal` on macOS), hosted as its own process by default.
   - The nodes: the research software's names, ports and descriptions, under ids `array.<slug>`.
   - The editor: an `--engine array` flag and an Engine menu entry.
   - Tests: parity with the research software on a few graphs.
3. **At source, in yapocis-rs:** remove its panics (an error type throughout), and check the 13 CPU replacements against yapocis_external where they overlap.
