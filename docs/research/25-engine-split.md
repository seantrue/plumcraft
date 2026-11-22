# 25. Splitting the built-in engines

Status: **thinking saved, not started.** The maintainer proposed the split on 2026-11-22 and then set its place in the order of work:

> Save the thinking on the refactoring of engines. We'll do it after the plummer development line can replace plumcraft, but before refactoring [the research software's PlumCraft port].

So this starts after the native development line (note 24) can replace the egui editor, and before the research software's port is refactored. Nothing here is decided beyond that order; the open decisions are listed at the end.

## The proposal

The maintainer's words:

> I'm also thinking we should split the engines into:
>
> array (generic array operations, including the curve editor, include the basic [the research software's GPU array library] array stuff, graphics, etc)
> audio, image, video -- encode/decode, round-trip to arrays, domain specific algorithms)
> tabular (rows/columns, headers, joins, splits, data operations, with round trip to arrays)
> text (what we have now, plus nodes for dealing with raw bytes, etc. Regular expressions)
>
> audio,image,video need the platform specific viewer support
> tabular should support graceful interop with future database engines

Six engines in place of three: **array, audio, image, video, tabular, text.**

## What exists (2026-11-22, the dev branch)

Counted from the node kinds named in each engine crate's source; a quick read, not a study.

| Engine | Crate | Node kinds | What is in it |
|---|---|---|---|
| Array | `crates/array` | 71 | Generic maths (arithmetic, FFT, convolve, slice, stack, generate), and also image codecs, colour, drawing, segmentation, contours, PDF and SVG rasterising, and video (GIF, MOV, MP4 to and from arrays) |
| Audio | `crates/audio` | 30 | Codecs (WAV, FLAC, AIFF), effects, analysis, round trip to arrays. Already its own engine; compiled into the native apps since 2026-11-22, a process for egui |
| Text | `crates/text` | 30 | Text operations, the table work (CSV, JSON to table, table to array and back), file formats (HTML, Markdown), and two testing nodes (`text.slow`, `text.fail`) |

The split is therefore mostly Image and Video out of Array, and Tabular out of Text. Audio already has the shape proposed.

## Where the kinds would go

Estimates.

| Engine | Kinds | From |
|---|---|---|
| Array | about 30 stay | Maths, FFT, generate, slice, stack, curves (`array.apply_curve` is already here) |
| Image | about 30 move | Decode, encode, load, colour space, draw, segment, contours, equalize, histogram match, PDF and SVG rasterising |
| Video | 6 move | GIF, MOV and MP4, each way |
| Tabular | about 10 move, plus new | The table kinds in Text; new joins, splits and data operations |
| Text | about 18 stay, plus new | New raw-bytes and regular-expression nodes |

## Impact, largest first

1. **Renaming node kinds breaks saved graphs.** `array.decode_image` becoming `image.decode` invalidates every saved graph, demo, fixture and test that names it: about 1,300 references in 180 files in this repository alone. No alias mechanism for node kinds was found (`aliases` in the protocol is for data handles). One is needed first, in the interface, so that old kinds keep loading.
2. **The Array and Image boundary needs a rule.** Blur, convolve, resize and rotate are array operations people use on pictures. A candidate rule: Array is anything that works on numbers whatever they mean; Image is anything that knows colour, codecs, files or drawing. About ten kinds sit on the line.
3. **Video has no core data type.** Video today is arrays of frames. A platform viewer needs something to view: a core type `VIDEO` beside `IMAGE` and `AUDIO` (golden rule 3 makes core types the maintainer's decision), and a native player in each shell.
4. **More graphs cross engine borders.** Decode, blur, encode becomes Image, Array, Image. Arrays are first class, so values cross unchanged, but the cost for large frames and video is unmeasured. Benchmark before committing.
5. **Packaging grows from three engines to six.** Six linked into the native apps, six programs beside the Windows app, a longer Engine menu; automatic engine picking has to stay invisible with six. Image and video codecs may bring dependencies that cannot be published, as the audio engine's did.
6. **Tabular and databases.** The DuckDB engine already feeds rows. The decision is the interchange: today's `TABLE` bytes, or a columnar format (Arrow, say) so database engines interoperate without copying.
7. **Tests and floor.** The conformance cases (63), the three floor tables in note 24, the Windows scene step files and the person checks all name today's kinds. With aliases they keep passing; without, all need editing.
8. **GPU array code in a built-in engine.** The research software's basic array kernels would bring GPU code into Array: fine on the Mac, needing a CPU fallback or another path on the iPad and Windows. Scope it separately.

## What helps

- **Save Result as a basic SDK feature** (decided 2026-11-22: "Make the save an basic feature."): each new engine declares its own save formats through the SDK.
- **Symmetric transforms**: every engine's round trip to arrays comes as a pair, which is what the Convert node (note 20) builds on. Convert's table of engine nodes must be re-pointed at the new kinds.
- **The launched checks** (note 24): the person checks and the test engine give a way to see that a split engine still works in a real app.

## A possible order

1. Save Result in the SDKs.
2. Node-kind aliases in the interface.
3. Text into Text and Tabular: the smallest split, and it proves the aliases.
4. Array into Array and Image: the largest move.
5. Video: the `VIDEO` type, the engine, the native viewers.
6. New nodes: bytes and regular expressions for Text, joins for Tabular, the GPU array basics.

## Open decisions

None answered yet.

| # | Question | The controller's suggestion |
|---|---|---|
| 1 | Old kind names: keep loading through aliases, or migrate saved graphs once? | Aliases |
| 2 | The Array and Image rule, and the ten or so boundary kinds | The rule above; the maintainer places the boundary kinds |
| 3 | `VIDEO` as a core type? | Needed if video is to have a viewer |
| 4 | Tabular's interchange: today's `TABLE`, or a columnar format? | Open |
| 5 | Where the engines live: crates in this repository, or separate engine repositories? | Open |
