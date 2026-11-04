# 20. Convert: one node between any two types that can meet

**Built 2026-11-03**, both slices, on branch `headless-write-files`. Section 5 says how it differs from this plan.

The maintainer, 2026-11-03:

- "Does it make sense to have a core node that will transform between all conformant data types. A connection on an input would constrain the output types, and vice versa. Like a result but with a xform."
- "Try that." (on writing this plan before building)

Prompted by wiring a SoundCraft bounce into an Audio input. That takes knowing that Bounce WAV gives a `WAV`, that the audio engine has WAV to Audio, and that the audio engine must be connected.

## 1. Today

- **A wire is accepted when the types match, or a `compat` rule allows it** (`docs/interface.md` 7.2, `SchemaReply::accepts` in `crates/proto/src/catalogue.rs:652`).
  - Compat rules are relabels: `INT → FLOAT`, `STRING → ENUM`, and the SDK's `T → ANY` / `ANY → T`.
  - No compat rule changes bytes.
- **Between engines, a value crosses only as a scalar, a core type, or a pack type both engines declare with the same media** (`portable`, `crates/coordinator/src/merge.rs:137`).
- **Conversions that change bytes are ordinary nodes:**

  | From | To | Node | Engine |
  |---|---|---|---|
  | `WAV` | `AUDIO` | WAV to Audio (`audio.from_wav`) | audio |
  | `AUDIO` | `WAV` | Audio to WAV (`audio.to_wav`) | audio |
  | `TABLE` | rows (`ARRAY` of `JSON`) | Table to Rows (`seq.table_rows`) | every SDK engine |
  | rows | `TABLE` | Rows to Table (`seq.rows_table`) | every SDK engine |
  | `TABLE` | `TEXT` (CSV) | Table to CSV (`table_csv`) | duckdb |
  | a craft document | `PDF`, `WAV`, `SVG`, … | the export nodes (`kind = "export"`) | each craft pack |

  Nothing marks them as conversions, so neither the editor nor the coordinator can find one.
- **Each conversion ships with its inverse** (maintainer). That is why most pairs come in twos.
- **A refused wire is just refused.** The person has to know which node bridges it, find it, and connect the right engine.

## 2. The design

### 2.1 Engines mark their converters (protocol: additive, a capability)

A node whose job is to turn one value into another says so in its `NodeDef`:

```json
"converts": {"from": "WAV", "to": "AUDIO", "lossless": true}
```

- **It applies to a node with exactly one wire-only input of type `from` and one output of type `to`.** Other inputs must be widgets with defaults: they are the conversion's options (bit depth, delimiter).
- **The engine may name which ports, for a node with more than one:** `"input": "wav", "output": "audio"`.
- **`lossless`** is the engine's claim that `to → from` gives the value back. It is used only to rank paths.
- **Capability `node.converts`.** An engine that doesn't send it has no converters, and nothing else changes. Hostile input: a `converts` naming a port the node lacks, or the wrong type, is dropped with a warning (the hostile sweep gets a case). The mark never fails the catalogue.
- **The SDKs:**
  - Python: `@node(converts=("WAV", "AUDIO"))`.
  - Rust: `def(...).converts("WAV", "AUDIO")`.
  - Craft packs: `converts = true` on an export node, its `from` being the pack's document type and its `to` the export's output.

### 2.2 The type graph (coordinator and session)

- Every connected engine's converters are edges of one graph. The coordinator already merges catalogues (`merge.rs`). The merged `SchemaReply` gets the union of `converts` marks, each with the engine that offers it.
- **A path is a list of converters**, with `portable` holding at every hop. A WAV made by SoundCraft reaches the audio engine only because `WAV` has the same media in both.
- **Ranking**, best first:
  1. fewest hops;
  2. then all lossless;
  3. then fewest engine crossings;
  4. then the catalogue's order.

  It is a breadth-first search over at most a few dozen types: no cost worth caching.
- **`compat` relabels stay free.** A path may begin or end with one (`INT → FLOAT`), so a converter's input accepts what its port accepts.

### 2.3 The first slice: Insert Convert on a refused wire

This gets most of the value with no new node kind.

- **When a wire is refused** for its types (`wire.connect`, or the drag ending on a port that won't take it), the editor looks for a path from the output's type to the input's.
- **One path: the refusal offers it.** The status line says "WAV can't go to an Audio input. Insert WAV to Audio (audio)?", and the drag's drop shows the same as a button.
- **The new command `wire.convert {from, to, path?}`:**
  - It places the converter nodes on the wire, laid out between the two ends, as one undo step.
  - `path` picks one of the paths, as `[{kind, engine}]`. Without it, the best is used.
  - Its result lists the nodes it placed.
- **A path through an engine that isn't connected** is offered as "Connect audio and insert WAV to Audio". This is the case that prompted this plan. The command then does an `engine.use` first. `check` and the banner already know which engine a node needs.
- **No path:** the refusal stays as it is today, and says no connected engine converts between them.
- **`plumcraft-editor check`** reports a refused wire with the path that would fix it, so a graph written by hand (or by an AI) learns its fix.

What is placed is ordinary nodes. The file and the run are as if the person had added them, so nothing about memo, expansion, saving or headless runs changes.

### 2.4 The second slice: the Convert node

A core node, `core.convert`, shown as **Convert**, with one input and one output, both untyped until wired. It is "like a Result" in the way the maintainer put it: core defines the slot; an engine supplies what runs.

- **Wiring constrains the other end.**
  - Input wired from a `WAV`: the output's type is "anything reachable from `WAV`". The canvas draws it as a list of types, and dragging from it offers only inputs of those types.
  - Output wired to an `AUDIO` input: the input accepts only what reaches `AUDIO`.
  - Both wired: the path is chosen (2.2) and **pinned in the workflow**:

    ```json
    "convert": [{"kind": "audio.from_wav", "engine": "audio"}]
    ```

  - Neither wired: it is a node with nothing to do, which `check` reports like any unused node.
- **Pinned, so a file runs the same way twice.** Connecting another engine doesn't reroute an existing Convert. Only `node.convert.repick {node}` does, or the pinned path vanishing, which is a problem `check` and the banner report, with the new best path offered.
- **Its options are the pinned converters' widgets.** They are merged onto the node, as the craft `*` widget merge does, under each converter's name when two have the same option.
- **When it runs, Convert expands into its converters** before the plan is split by engine. A stored expansion is expanded the same way, so the run, the memo and the coordinator see only engine nodes. Its watched value and badge are the last converter's.
- **The node shows its path:** `WAV → Audio · audio` in the header, each hop's engine as the badge does now.
- **`ANY` is not widened.** `ANY` passes a value through untouched. Convert changes it, and a port typed `ANY` never matches a Convert end by the `ANY` rule alone: it takes the type of the far end.

### 2.5 What isn't done

- **No conversion code in core** (AGENTS rule 3). Convert has no `run`. With no engine offering a path it can't run, and says so.
- **No guessing between unrelated types.** A path exists only through marked converters. `TEXT → TABLE` is a parse that some engine has to offer, not something core infers.
- **No implicit insertion.** A refused wire is never silently fixed. The person (or the command's caller) picks.

## 3. Order of work

| Step | What | Where | Tests |
|---|---|---|---|
| 1 | `converts` on `NodeDef`; capability `node.converts`; dropped when it lies | `crates/proto`, `docs/interface.md`, schema regenerated | hostile sweep; serde round trip |
| 2 | Marks on the existing converters: `audio.from_wav`/`to_wav`, `seq.table_rows`/`rows_table`, DuckDB's `table_csv`, the craft exports | `crates/audio`, `crates/engine` (seq), `../plumcraft-duckdb`, `crates/craft/packs/*.toml` | each catalogue lists them |
| 3 | The type graph and path search over a merged catalogue | `crates/coordinator` (or `crates/graph`, so a single engine has it too) | WAV→AUDIO through two engines; ranking; a non-portable hop refused |
| 4 | `wire.convert`; the refusal offering it; connecting the engine first | `crates/session`, `crates/ui` | session unit; kittest: drag Bounce WAV onto an Audio input, accept, run |
| 5 | `check` naming the fix | `apps/plumcraft-editor/src/headless.rs` | headless test |
| 6 | `core.convert`: two-way constraints, pinned path, merged options, expansion before the split | `crates/proto` (core kind), `crates/session`, `crates/coordinator`, `crates/ui` | the ringtone graph with Convert in place of WAV to Audio, run headless and in the window |

Steps 1 to 5 are the first slice and stand on their own. Step 6 follows only if the first slice shows the node is wanted, rather than the insertion alone being enough.

## 4. The maintainer's answers (2026-11-03)

1. **Both slices:** Insert Convert on a refused wire, and the Convert node.
2. **The path names only the kinds; the engine is left to the coordinator's usual choice.**
3. **A lossy path gets a warning, not a confirmation.**
4. **Craft exports are converters**, so a craft document reaches a value in two steps ("two steps for consistency"): SoundCraft doc → Bounce WAV → WAV to Audio.

## 5. As built (2026-11-03)

- **The mark is an optional field, not a capability.** `converts` is optional and ignored by a UI that doesn't know it, as `editor` is. A capability would only have repeated it.
- **Where the code is:**
  - `plumcraft_proto::convert`: the mark, the checked converters, the path search, and the Convert node's definition, which `SchemaReply::node` always finds.
  - `plumcraft_graph::convert`: the two-way type rules, the pin, and the pin problem.
  - `plumcraft_session::convert`: `wire.convert`, `wire.convertOffers`, `node.convert`, the expansion before every run and save, and the translation of what comes back.
  - The editor: the offer beside a refused wire, and the path in a Convert node's footer.
  - `check`: names the fix.
- **Marked:**
  - audio's WAV to Audio and Audio to WAV (lossless);
  - audio's Audio to Array and Array to Audio, added for this. The first isn't lossless, since the sample rate leaves by a second output; Array to Audio takes it back, 48 kHz unless wired. A Convert from Array to Audio can't wire it, so it uses 48 kHz;
  - every craft export (not lossless);
  - DuckDB's Table to Rows and Rows to Table (lossless) and Table to CSV.
  - The SDK's `seq.table_rows` and `seq.rows_table` are `ANY` → `ANY`, so they are not marked.
- **Tried:** `claude-debug/convert/ringtone-convert.json` is the ringtone graph with a Convert wired straight from the session document into Loudness. It passes `check` and runs across SoundCraft and the audio engine as Bounce WAV, then WAV to Audio.
- **Tried across three engines:** `claude-debug/convert/ringtone-array.json` takes the session through a Convert into the array engine's Arithmetic (× 0.5), then through a second Convert back to Audio and Loudness. The path is Bounce WAV → WAV to Audio → Audio to Array, then Array to Audio. It reads −31.51 LUFS against −25.49 unhalved: 6.02 dB, as it should.
- **Not done:**
  - **Options.** A Convert's steps run with their options' defaults. The ringtone's own Bounce WAV normalises; the Convert's doesn't, so the two loudness readings differ (−25.5 against −15.6 LUFS). Showing the steps' options on the Convert is the next step.
  - **An engine not connected** isn't offered. The editor has no catalogue for an engine it hasn't started, so "Connect audio and insert WAV to Audio" (2.3) waits for engines to publish their marks before they start (in their manifests, say).
  - **Lists.** A Convert's wires skip the list rules; a list into it maps its steps as the engine maps any node.

## 6. Converters still to implement (2026-11-03)

The maintainer asked "What converters need to be implemented?", then, of video, "Check commonkit tests for an example of video as a 4d array" and "add to the list".

### 6.1 Nodes that existed but weren't marked: done 2026-11-03

The maintainer: "mark them, and ensure that arrays can be non-images (1d)".

| Node | Converts | Lossless |
|---|---|---|
| `array.decode_image` | IMAGE → ARRAY (`image`) | yes |
| `array.encode_image` | ARRAY (`image`) → IMAGE | no (8 bits, clipped to 0–1) |
| `array.rasterize_svg` | SVG → ARRAY (`image`) | no |
| Each craft's Open, one mark for each of its `image`, `svg` and `pdf` inputs | IMAGE, SVG or PDF → that craft's document | no |

PdfCraft's Open and Export text take a PDF to text in two steps.

**Arrays that aren't pictures.**

- **Marks can say an ARRAY's shape** (`from_shape`, `to_shape`): `image` or `series`. Audio to Array gives a `series` and Array to Audio takes one.
- **A path never crosses shapes.** A sound never becomes a picture through a Convert, and a picture never becomes a sound. `check` says "no connected engine converts AUDIO to IMAGE".
- **The array engine kept turning a 1-D array into a 1 × n one.** It reads a 1-D array as one row, so its nodes gave back `(1, n)`. Its node runner now puts such a result back to `[n]` when the node was given a 1-D array of that length (`keep_flat`, `crates/array/src/op.rs`). A spectrum stays a spectrum through Arithmetic, and Array to Audio takes it as mono.
- **A node may carry several marks** (`converts` is one or a list).

### 6.2 File types that are dead ends

Craft exports make these, and nothing takes them further.

| From | To | Note |
|---|---|---|
| `TXT`, `MD`, `HTML` | TEXT | a decode |
| `CSV` | TABLE | a parse |
| `FLAC`, `AIFF` | AUDIO | value nodes like WAV to Audio; the audio engine already decodes them from files |
| `PDF` | IMAGE | rasterise a page |

### 6.3 Between core types

| From | To | Note |
|---|---|---|
| TEXT | TABLE | CSV parse; the inverse of DuckDB's Table to CSV |
| JSON | TEXT, and back | serialise and parse |
| ARRAY | TABLE, and back | a 2-D table array as named columns |

### 6.4 Video as a 4-D array

commonkit's `tests/test_ffmpeg.py` (in `../pycommonkit`) shows the convention:

- ffmpeg renders raw frames, read as a `(T, H, W, C)` array: `rgb24` is `uint8` with 3 channels, and `grayf32le` is `float32` with 1.
- The frame rate is outside the array. It goes in the key's variant (`fr10`, `fr25`), as `sr16000` carries a sample rate.
- One frame is a view.
- A clip over the size limit is declined whole, never truncated.

So video needs no new core type. PlumCraft's ARRAY already takes N dimensions, and its viewer gives them a slice picker. The pair mirrors Audio to Array and Array to Audio:

| Converter | From → to | Note |
|---|---|---|
| Video to Array | `MP4`, `MOV`, `GIF` → ARRAY `(T, H, W, 3)` `uint8` | the frame rate as a second output, as Audio to Array gives the sample rate; not lossless (the encoding is gone) |
| Array to Video | ARRAY `(T, H, W, C)` → `MP4` | the frame rate an input; a lossy encode |

They would let FilmCraft's exports (`fc.export_mp4`, `fc.export_mov`, `fc.export_gif`) reach the array engine through a Convert. A frame, `[t]`, is then an `(H, W, 3)` picture.

Open questions:

- **The engine:** the array engine is the natural home, with ffmpeg on PATH; without it, a hint, as the commonkit test skips.
- **A size limit:** about 1.4 MB a second at 160×120 RGB, but gigabytes for HD. Refuse rather than truncate, as commonkit does.
- **The soundtrack:** possibly a third output, as Audio.

### 6.5 Probably never

- **MIDI, MusicXML → Audio:** needs a synthesiser, a choice rather than a conversion.
- **3D files** (STL, STEP, 3MF, OBJ): no core type to go to.
- **Spectrum, Spectrogram and Array to SVG:** analyses and plots, so they stay unmarked.

### 6.6 Suggested order

1. ~~The marks in 6.1.~~ Done.
2. `TXT` and `CSV` to TEXT and TABLE, and `FLAC` and `AIFF` to AUDIO (6.2): most craft exports then lead somewhere.
3. The core pairs (6.3).
4. Video (6.4).
