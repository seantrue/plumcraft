# OpenCV engine review

*2026-11-20. The repository reviewed is `/Users/seant/workspace/opencv-engine` (remote `seantrue/plumcraft-opencv-engine`), at `a0fd881` on `main`. Nothing was changed there. The tests ran in a copy at `claude-debug/opencv-review/repo`, with a scratch commonkit store and `COMMONKIT_HASH=b3`.*

## Verdict

**The engine works well and is safe to recommend for desktop image workflows with a few caveats. It is not yet complete enough to call "the OpenCV engine".** Its 61 OpenCV nodes (75 counting the SDK's building blocks) cover filters, morphology, thresholds, colour, basic geometry and the photo module well. It passes every conformance case: 14 of 14 on the Python side and 18 of 18 on the Rust side. It speaks the core types (`ARRAY`, `IMAGE`, `TABLE`) with the same array conventions as the built-in array engine. The biggest gaps are these:

- perspective geometry and shape measurement;
- feature matching and detection;
- video beyond one frame;
- alpha and 16-bit fidelity;
- progress and cancel on the long nodes (Grab Cut takes 31.6 s at 2048² and can't be stopped).

**Bindings:**

- **Python:** exists. It is the whole engine, one 889-line module on `opencv-python-headless==4.14.0.94`.
- **Rust:** does not exist. Nothing in the repo or its history plans one. PlumCraft's plan (`docs/plan.md`, decision D16) recommends the Python package *instead of* the Rust `opencv` crate, because of "system OpenCV, libclang, slow and fragile builds".

## 1. What the engine is

- **Language and hosting:** Python 3.12 on the PlumCraft Python SDK (`plumcraft-engine` 0.2.0, git pin `34f6aea`, which is the commit tagged `plumcraft-v0.7.0`).
  - The manifest's mode is `process`: `uv run --project . python -m opencv_engine`, restart on crash, at most 5 restarts.
  - It also serves HTTP (`--listen`).
  - `opencv-engine register` writes a user manifest.
- **Dependencies:**
  - `opencv-python-headless` (pinned, and named in `dependencies` so its version joins the memo salt);
  - numpy, `pyuncommonkit`, `blake3`, and pillow (pulled in by the SDK).
- **Determinism:** OpenCL is off, and seeds are widgets on K-Means and Grab Cut.
- **Catalogue:**
  - 61 `opencv.*` nodes in 13 categories, plus the SDK's building blocks (`file.*`, `vary.*`, `seq.*`).
  - Wire types: `ARRAY`, `IMAGE`, `TABLE`, `ANY` and the scalars.
  - Pictures are ARRAY float32 0–1, H×W×3 RGB (`space: "RGB"`, names red/green/blue) or H×W. BGR and uint8 stay inside the module.
- **Tests:** 50 pytest cases, in five files:
  - nodes (36);
  - round trips (8);
  - golden catalogue, demo and cartoon group (3);
  - the hash under md5 and b3 (2);
  - conformance (1).
- **Examples:**
  - `examples/demo.json` has 56 nodes and runs every OpenCV node except Load Image and Video Frame;
  - `examples/cartoon.json` builds the Cartoon node out of its separate steps.
- **README and AI.md claims:** the README's claims checked out with the exceptions in section 6; AI.md's claims checked out except one stale line.

| Category | Nodes |
|---|---|
| Files | Test Picture, Load Image, Encode Image, Decode Image, Video Frame |
| Colour | To Grey, To Colour Space, From Colour Space (HSV, Lab, YCrCb), Split Channels, Merge Channels, Equalize Histogram, Normalize, Convert Scale, CLAHE |
| Filters | Gaussian Blur, Median Blur, Bilateral Filter, Laplacian, Sobel, Box Filter, Filter 2D, Scharr |
| Edges | Canny, Threshold (fixed/otsu/triangle), Adaptive Threshold (mean), In Range |
| Morphology | Morphology (7 operations), Structuring Element, Distance Transform, Connected Components |
| Geometry | Resize, Rotate (quarter turns), Flip, Pyramid Down, Pyramid Up, Crop, Warp Affine |
| Features | Hough Lines (P), Draw Segments, Find Contours, Draw Contours, Good Features, ORB Keypoints, Draw Points |
| Photo | Stylization, Pencil Sketch, Detail Enhance, Edge-Preserving Filter, Inpaint, Denoise, Cartoon |
| Segmentation | K-Means Colours, Watershed, Grab Cut |
| Composite | Add Weighted, Abs Diff, Bitwise (and/or/xor/not, mask) |
| Testing | Slow (progress and cancel) |

## 2. The two bindings

**Python: exists and is sound.**

- It holds every node, and its tests cover every node.
- Errors are readable: a `cv2.error` becomes an `op` error carrying OpenCV's last line.
- It avoids `cv2.imread`, which segfaults on a bad path; files are read in Python and passed to `imdecode`.
- It relies on running as a process: a crash in OpenCV's C++ kills the process, and the host restarts it.

**Rust: does not exist.**

- There is no Cargo file, no branch (`main` and `sdk-windows` only) and no mention in six commits.
- PlumCraft's plan lists the OpenCV engine as Python (modes 3 and 4).
- D16 rejects the `opencv` crate.

**What a Rust binding would take, and whether it is worth it:**

| Factor | The `opencv` crate (binding the C++ library) | Pure-Rust ports into the array engine (`imageproc` and the like) |
|---|---|---|
| Build | System or vendored OpenCV, plus libclang for bindgen; long builds; CI on Linux needs `libopencv-dev` of the matching version | Ordinary crates; the existing workspace build |
| iPad / Mac compiled in | OpenCV cross-built as static iOS libraries; tens of MB added to the app; fragile | Works where the array engine already works |
| Golden rules | Every call is FFI under the hood. A C++ segfault in process takes down the app, which goes against "never crash"; the process host's restart covers only an out-of-process engine | No `unsafe`, no C++ |
| Licence | OpenCV 4.5+ is Apache 2.0: compatible | MIT/Apache crates |
| Performance | Same kernels as Python. Python's overhead here is small: the float32/u8 conversions and copies cost more than the interpreter | Usually slower than OpenCV's SIMD kernels, but enough at editor sizes |

**Recommendation:**

- Don't bind the C++ library from Rust.
- Keep the Python engine for desktop and remote use.
- When the iPad needs OpenCV-grade operations, port the most-used ones (perspective warp, contour analysis, template matching, Otsu, distance transform) to pure Rust in `crates/array`.
- The iPad can't spawn a Python process. It could reach this engine only over HTTP on another machine; I did not check whether the iPad app offers Attach.

## 3. Completeness against `docs/interface.md`

Most of the protocol belongs to the SDK, which this engine inherits. The table marks which is which.

| Requirement (section) | State | Notes |
|---|---|---|
| hello, protocol, caps, `data.algos` (5.2, 4.3) | Meets (SDK) | One algorithm, b3 by default; `test_hash.py` checks md5 and b3 |
| Same-host proof, session.confirm (5.4) | Meets (SDK) | |
| Off-run-path answers (5.5) | Meets (SDK) | Conformance "answers off the run path" passes, Python and Rust sides |
| Catalogue, NodeDef (7.1, 7.3) | Meets | Golden catalogue test guards port names |
| Core types declared exactly (7.2) | Meets | ARRAY, IMAGE, TABLE; no opaque pack types |
| Array conventions (7.2) | Partly | float32 0–1 RGB with `space` and names, as `crates/array`. **But `rgb()` stamps `space: "RGB"` on every 3-channel output, so a blurred or resized Lab or HSV array is labelled RGB.** RGBA (4 channels) is refused |
| Widgets, ranges, choices (7.3) | Meets | `Range(..., invalid="clamp")` throughout; ENUMs for modes. Colour ports are STRING `#rrggbb` without `widget: "hexcolor"`, so there is no colour picker |
| `converts` marks (7.3, 7.4) | Does not | No node is marked. Encode and Decode Image are natural ARRAY↔IMAGE converters; the editor finds the array engine's instead |
| Inverses for conversions (maintainer's rule) | Meets | Encode/Decode, To/From Colour Space, Split/Merge, Rotate, Flip and the Pyramids, all round-trip tested. To Grey says why it has none |
| Graph semantics, lists, slices, bindings (7.6) | Meets (SDK) | Conformance passes |
| Status states, ms, preview before downstream (9.3, 9.4) | Meets (SDK) | "record invariants" passes |
| Progress (9.4) | Partly | Only `opencv.slow` reports progress. Grab Cut, K-Means, Denoise and Cartoon report none |
| Cancel inside long nodes (11.3) | Partly | Between nodes, yes (SDK). Inside a node, only Slow. Grab Cut took 31.6 s at 2048² with no way to stop |
| Displays, renditions, stats, thumbs (10) | Meets (SDK) | Plus `picture_thumb` for Load Files values |
| Live data, `node.live` (10.6) | Does not | Not in the SDK; nothing here would need it except iterative Grab Cut |
| `output.save` / `save_formats` (15) | Does not | Not in the Python SDK. Saving goes through Encode Image then Write Files |
| Memo, salt, persistence (14) | Meets (SDK) | `dependencies` puts the OpenCV version in the salt. No node sets `version`; the code digest covers it |
| Errors config/op (12.1) | Meets | Readable. Two rough edges: "a picture with 1 channels, not 64×64×3" when Threshold gets RGB (no hint to use To Grey), and a raw `OverflowError ... out of bounds for int32` for huge contour coordinates |
| Hostile input (18) | Partly | Hostile widget values pass, and every bad table or array I tried gave an error, not a crash. **Decode limits are missing:** Load Image accepted a 20000-pixel-wide PNG; section 18 asks for at most 16,384 a side, checked before allocation |
| Shutdown, restart, kill -9 (16.3) | Meets | "kill -9 mid-run recovers" passes |
| HTTP binding, token (16.4) | Meets | "a wrong token is refused" passes |
| Manifest and register (16.6) | Meets | `[conformance] slow = "opencv.slow"` |
| pipekit mode 4 (plan) | Not applicable | The host doesn't support pipekit yet |

## 4. Completeness against OpenCV

| Area | There | Missing | Matters |
|---|---|---|---|
| I/O and codecs | Load (png/jpg/tif/webp), Encode (png/jpeg/webp), Decode | Alpha, 16-bit and float (EXR/HDR) depth, quality options, multi-page TIFF, a size limit | High |
| Colour | Grey, HSV/Lab/YCrCb both ways, split/merge, equalize, CLAHE, normalize, gain/offset, In Range | HLS, Luv, XYZ (the array engine has them), colour maps (`applyColorMap`), LUT/gamma, white balance | Medium |
| Geometric | Resize, quarter turns, flip, pyramids, crop, affine (angle/scale/shift) | **Perspective warp from four corners**, homography, remap, pad (`copyMakeBorder`), rotation onto a larger canvas, undistort | High |
| Filtering | Gaussian, median, bilateral, box, Filter 2D, Laplacian, Sobel, Scharr | Unsharp mask, gradient magnitude and angle, Gabor | Low |
| Morphology | All 7 `morphologyEx` operations, element, distance transform, components | Components with stats (areas, centroids), thinning, custom element input | Medium |
| Thresholding | Fixed, Otsu, triangle, adaptive mean | Adaptive Gaussian, trunc/to-zero | Low |
| Edges and contours | Canny, contours (area, perimeter, box), draw | Hierarchy, polygon approximation, convex hull, minimum-area box, ellipse fit, moments and centroids, shape match, Hough circles | High |
| Histograms | Equalize, CLAHE | Histogram as a table, compare, back-projection | Low (array engine has histogram and matching) |
| Features and matching | Shi-Tomasi corners, ORB keypoints | Descriptors, matching, RANSAC homography, SIFT/AKAZE, **template matching**, optical flow, alignment, stitching | High |
| Drawing | Segments, contours, points | Rectangles (from boxes), text, ellipses, polygons | Medium |
| Arithmetic and blending | Add Weighted, Abs Diff, Bitwise with mask | Multiply/divide/min/max, **`seamlessClone`** | Medium |
| Masks and segmentation | In Range, thresholds, Grab Cut (box), Watershed, K-Means, Inpaint | Flood fill, mean shift, superpixels, the core mask editor on Inpaint (which the plan intended) | Medium |
| Photo | Stylization, pencil sketch, detail enhance, edge-preserving filter, inpaint, colour denoise, cartoon | HDR merge and tonemapping, grey and multi-frame denoise | Medium |
| Video | One frame at a time in seconds | Frames as (T, H, W, C), writing video, fps and length, background subtraction, tracking | Medium (array engine reads and writes mp4) |
| Camera calibration and 3D | None | Chessboard, calibration, undistort, solvePnP | Low for image workflows |
| Detection and DNN | None | **QR codes and ArUco** (in main OpenCV, no model needed), Haar faces (cascades ship in `cv2.data`), DNN/ONNX | High |

**Estimate:** about a third of what someone building image workflows would expect.

- Strongest: imgproc filters, morphology and photo.
- Weakest: geometry, measurement, matching, detection and video.

**Fit with the array engine:** the conventions agree.

- Both use float32 0–1, `space: "RGB"` with names red/green/blue, HSV hue 0–360 and Lab L 0–100.
- The array engine's Load and Decode give `rgb`, `alpha` and `rgba` outputs. Its `rgba` can't feed any OpenCV node, which refuses 4 channels.
- Encoding differs by rounding: the array engine truncates, OpenCV rounds by adding 0.5.
- They overlap on Gaussian blur, resize, rotate, equalize, normalize, connected components, colour conversion, Sobel, median, contours, split/stack and encode/decode.
- I did not run the two engines side by side to compare their numbers.

## 5. Usefulness

You can build real workflows today. Typical ones:

- a cartoon or stylized photo;
- cleanup by threshold, morphology and contours;
- background removal with Grab Cut;
- colour quantising;
- inpainting a mark.

Node grain is right, names are plain, and defaults run on the test picture. The contour, line and point nodes give tables that their Draw nodes take back.

**Traps found by probes** (`claude-debug/opencv-review/probe_traps.py`, log beside it):

- **Silent 8-bit quantising and clipping.** Median Blur, Equalize, CLAHE, Bitwise, Inpaint, Denoise and the photo nodes go through uint8 without saying so in their descriptions:
  - a signed Laplacian into Median Blur loses every negative;
  - a 0–255 picture comes out as 0/1;
  - HDR input over 1 is clipped;
  - a gradient of 4096 levels comes out of Equalize as 2.
- **Space mislabelled.** A Lab array through Gaussian Blur or Resize comes out labelled RGB, so the viewer and To Colour Space take it for RGB.
- **Alpha dropped.** A PNG with alpha loads as 3 channels, and RGBA arrays are refused.
- **16-bit dropped.** A 16-bit PNG loads with 256 levels.
- **Long and unstoppable.** At 2048×2048:

| Node | Time |
|---|---|
| Grab Cut, 5 iterations | 31.6 s |
| K-Means, 16 colours, 3 attempts | 2.3 s |
| Stylization | 0.7 s |
| Denoise | 0.4 s |

  None reports progress or stops on cancel.
- **No decode limit.** A 20000-pixel-wide PNG loaded.
- **No mutation in place.** Inputs are copied before drawing, so that is not a trap.

**Five most valuable additions or fixes:**

1. **Progress and cancel in long nodes.** Run Grab Cut one iteration at a time (`GC_EVAL`), K-Means one attempt at a time, and Cartoon one pass at a time, calling `cx.progress` and `cx.cancelled()` between steps.
2. **I/O fidelity and limits:**
   - read with `IMREAD_UNCHANGED`: 16-bit as /65535, and an alpha output as the array engine gives;
   - accept RGBA when channel 4 is named `alpha`;
   - check the header against 16,384 a side and 256 Mpx before decoding;
   - add a quality option, and 16-bit PNG/TIFF on Encode.
3. **Honest ranges and spaces:**
   - keep the input's `space` and names on per-channel filters (fix `rgb()`);
   - say "8-bit" in the description of every node that quantises, and refuse or warn when its input lies outside 0–1;
   - a Threshold error that names To Grey;
   - `widget="hexcolor"` on colour ports;
   - `converts` marks on Encode and Decode Image.
4. **Geometry and measurement:**
   - Warp Perspective from four corners, with its inverse;
   - contour shape analysis (polygon approximation, hull, minimum-area box, moments and centroids, hierarchy);
   - components with stats as a table;
   - Template Matching;
   - Draw Boxes and Draw Text.
5. **Detection and matching:**
   - QR code and ArUco detection;
   - Haar faces, from the cascades in `cv2.data`;
   - ORB/SIFT descriptors with matching and a RANSAC homography, for aligning one picture to another.

## 6. Health

- **Pin:** `34f6aea`, which is `plumcraft-v0.7.0`, the current `origin/main`.
  - Since then on `origin/feat/native-n0`: `docs/interface.md` is byte-identical, and nothing changed in `crates/proto`.
  - In `python/`, one commit (`dd01c26`) adds a test node to the *text* engine. Nothing affects this engine, so the pin is current.
- **Rust-side conformance against the current host:** 18 of 18. This ran with a copy of the dev branch's `target/debug/plumcraft` (built 2026-11-20 by the other agent; not built by me). The installed `~/.cargo/bin/plumcraft` is 0.1.0, too old for the 0.7.0 hello.
- **Stale documentation:**
  - The README says the SDK is pinned to the `headless-write-files` branch; it is the 0.7.0 tag.
  - The README's `scripts/run --watch parsed.float` names a node the demo doesn't have.
  - AI.md says the repo is private; it is public.
- **No CI:** there is no `.github`, so the tests run only by hand. A Linux-only workflow would be cheap.
- **No release:** version 0.1.0, no tags, not on PyPI. Users install from git.
- **Code quality:** one tidy module with consistent helpers and good docstrings.
- **Missing tests:**
  - RGBA and 16-bit input;
  - out-of-range input to the 8-bit nodes;
  - decode limits;
  - space labels kept through filters;
  - Bitwise `not`;
  - huge coordinates in tables.
- **Golden rules, as they apply to an engine:**
  - It makes no hashes of its own, and has no opaque or hard-coded types.
  - Hostile input gives errors, not crashes, except the missing decode limit.
  - A C++ crash relies on the process host restarting it, as the README says.

## 7. Test results

| Run | Result |
|---|---|
| `ruff check`, `ruff format --check` | Clean (10 files) |
| `pytest` | 50 passed in 48.4 s, 0 failed |
| `plumcraft-engine check` (Python half) | 14 of 14: process and HTTP × session, core types, every node on defaults, demo, cancel mid-run, off-run-path answers, hostile widget values |
| `plumcraft conform` (Rust half, dev-branch host) | 18 of 18: process and HTTP × hello and schema, record invariants, off-run-path, cancel mid-run, cancel before its run, delivery equivalence, resume from seq, memo reuse; kill -9 recovery (process); wrong token refused (HTTP) |

No process was left running.

## 8. Not checked

- Windows, Linux, and installing from GitHub with `register`.
- The editor GUI: no app was launched, by the rules.
- Numeric agreement with the array engine: no Rust build was run.
- Video Frame on real codecs (only its own test).
- Memory behaviour near the 256 M-value cap.
- Whether the iPad app can attach to an HTTP engine.
- Conformance against a host built at exactly 0.7.0: I used the dev branch's, whose interface is identical.
