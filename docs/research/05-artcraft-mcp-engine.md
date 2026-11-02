# 05: An engine built on an artcraft app's MCP server

*2026-10-27. Research note for the generic-base study (branch `generic-base-research`). Survey of `~/src/artcraft` (photocraft excluded). It builds on `docs/research/flocraft_effort_20261017.md`.*

## Conclusion

- **The primary candidate is SoundCraft**, and **VectorCraft is the second**. SoundCraft gives the audio example engine for free and forces node visualisation beyond images (waveform, spectrum, meters). Its `engine.plugins` query returns a *typed* parameter catalogue (min, max, default, unit, taper, choices), so the palette can be generated with no hand-written Rust. VectorCraft turns rasters into vectors (`trace`: contour, centerline, mosaic, quantize) and writes SVG/PDF/PNG. That complements the research software supporting www.virtimp.com directly (potrace output, cartoon vectorisation), and its `run` already chains steps with `$N` references.
- **Every craft speaks the same command model** (`id, label, menu, shortcut, params, enabled, run`), which is the same "everything is a command" rule the research software follows. So one generic **craft-MCP adapter** covers all fourteen apps. Each app then only needs a small manifest: which wire types its documents and exports produce, and which commands are pure.
- **Craft commands are not pure functions.** They mutate one open document (session, drawing, text document) and often act on the current *selection*. Dataflow needs one of three mappings (below). The adapter makes them pure by **threading a document handle (a CAS key) through the wires** and making selection an explicit parameter.
- **MCP is a usable transport, but not a dataflow protocol.** It has no typed parameter schema (the `params` field is a prose string), no per-node status and no shared memory. The adapter supplies all three, and it is the only place that knows about MCP. Two small fixes at source in the crafts would make the adapter far more exact: a JSON Schema `params_schema` and an `effect` class on `CommandSpec`. A third would help audio: `.npy` import/export, or a pure `dsp.process`.

## What was surveyed

The counts are approximate. They come from regex over `crates/engine/src` (`craft_survey.py` in the session scratchpad), plus `<app>-cli commands` where it was run.

| App | Domain | Commands (approx.) | MCP crate | Headless CLI (`run`/`convert`) | Release build present | Fit as a node engine |
|---|---|---|---|---|---|---|
| soundcraft | DAW (audio, MIDI) | 387 | `crates/automation` (hand-rolled JSON-RPC, stdio) | yes: `run --in S.scraft --cmd ID=JSON --bounce OUT --save`, `convert`, `info`, `describe`, `plugins` | yes | **Best**: typed DSP catalogue, offline DSP in `soundcraft-dsp::offline` |
| vectorcraft | Illustrator-class vectors | 377 | `crates/mcp` (progress + cancel) | yes: `run --in F --cmd … --export F`, `$N` refs, `convert`, `info` | yes | **Second**: raster→vector trace, pathops, SVG/PDF out |
| wordcraft | Word-class documents | 642 (CLI listing) | `crates/mcp` | yes: `convert`, `text`, `inspect`, `render PNG`, `run --script` | yes | Good for a *text* engine: `convert`/`text`/`render` are pure |
| lightcraft | Lightroom-class photos | 224 | `crates/mcp` (progress, `explicit_targets`) | yes | yes | An image domain again, so less value for generalising; catalogue-centred |
| effectcraft | After Effects-class | 202 | `crates/automation` (`long_job.rs`: progress + cancel for renders) | yes | yes | Heavy project state; video output |
| filmcraft | NLE | 93 | `crates/automation` (uses **rmcp**; long jobs) | yes | yes | Timeline state; video |
| designcraft | InDesign-class | 313 | `crates/mcp` | yes | yes | Layout documents; page renders |
| deckcraft | slides | 210 | `crates/mcp` | yes | yes | Document-only |
| gridcraft | spreadsheet | 112 | `crates/mcp` | yes | yes | Interesting for *tables*: formula/pivot nodes over a TABLE type |
| figcraft | Figma-class | (different macro) | `crates/mcp` | yes | yes | The MCP server is live in this session (headless backend); design documents |
| cadcraft, solidcraft | 2D/3D CAD | (different macro) | `crates/mcp` | yes | solidcraft: no release build | Meshes and solids: a future 3D wire type |
| mailcraft, pdfcraft | mail, PDF | (different macro) | yes | yes | yes | Not a dataflow fit |

Side finding: Claude Code's MCP config points the craft servers at `/Volumes/src/artcraft/<app>/target/release/<app>-cli`, which doesn't exist. The binaries are under `/Users/me/src/...`, so every craft MCP server except figcraft failed to connect in this session.

## The common craft command model

Every app has the same registry. Here is soundcraft's (`crates/engine/src/cmd/mod.rs`):

```rust
pub struct CommandSpec {
    pub id: &'static str, pub label: &'static str,
    pub menu: &'static [&'static str], pub shortcut: Option<&'static str>,
    pub params: &'static str,              // prose doc: "{process: <plugin id> | normalize | …, params?: {..}, clips?, …}"
    pub enabled: fn(&Engine) -> Result<(), String>,   // Err(reason) = disabled
    pub run: fn(&mut Engine, &Value) -> Result<Value>,
    pub journal: bool, pub undoable: bool, pub view: bool,
}
```

The apps differ only in the side flags. These flags are the raw material for deciding which commands are node-safe:

| App | Flags that say what a command touches |
|---|---|
| soundcraft | `journal` (false = query), `undoable`, `view` (only touches zoom) |
| wordcraft | `mutates` (false = `.pure()`: navigation, view, export) |
| lightcraft | `journal`, `explicit_targets` (a call that names its photos skips the selection check) |
| others | `journal` / `query` macro variants |

This matches the research software's golden rule 3 exactly: id, label, menu path, shortcut, params doc, `enabled`, `run`. The research software's own engine commands (`add node`, `connect`, `run`) are *editor* commands, while a craft's commands are *domain* operations. In the generic base, both live in one registry type. Editor commands drive the graph. Domain commands become **node kinds** through an engine adapter.

**Proposed shared type (craft-libs):** add two fields to every craft's `CommandSpec` and to `CommandInfo` as `list_commands` returns it:

- `params_schema: &'static str`: a JSON Schema for the params, kept next to the prose `params`. A test checks that every command's schema parses and that its examples validate.
- `effect: Query | Mutate | View | Ui | Io`. `Query` is pure and reads the document. `Mutate` changes the document and is deterministic given params. `View` and `Ui` are never nodes. `Io` reads or writes files, so it is confined by the automation roots.

The flags that exist today (`journal`, `undoable`, `view`, `mutates`) map onto `effect` mechanically, so the change is a sweep in each repo. Per golden rule 7, it is proposed at source, not worked around.

## MCP surface (what the adapter talks to)

All crafts serve newline-delimited JSON-RPC 2.0 on stdio (`<app>-cli mcp`), protocol versions 2024-11-05 to 2025-06-18 (effectcraft also handles 2026-07-28). They expose a few *meta* tools, not one tool per command:

- `list_commands {filter}` → `[CommandInfo]`
- `execute {command, params}` → the command's JSON result
- `batch {calls[≤1000], keep_going}`
- `inspect_session` / `inspect_document` → the document as JSON
- `new_session` / `open_session` / `save_session`, plus import and export tools (`import_audio`, `bounce_mix`, `export`, `screenshot`)
- resources such as `soundcraft://session` and `soundcraft://commands`
- `--automation-read-root/--automation-write-root DIR`, which confine every file the commands read or write. The adapter points these at the CAS and commonkit directories.

Progress and cancellation (`_meta.progressToken` → `notifications/progress`; `notifications/cancelled`) are implemented in vectorcraft, lightcraft, effectcraft and filmcraft (for long jobs). soundcraft's server doesn't implement them, and wordcraft's ignores them.

Because the tools are generic (`execute` by id), **the adapter never needs per-command MCP glue**: it reads `list_commands` once and dispatches by id. That is the craft equivalent of the research software's schema-driven palette.

## How commands map to nodes

The adapter offers three node shapes, chosen per command by a manifest (generated, then hand-trimmed).

### A. Catalogue nodes (typed, pure)

These are the cleanest. They come from a query that returns typed descriptors, not from the command list. For soundcraft, `engine.plugins` returns 28 plugins (EQ 1-band and 7-band, compressor, gate, de-esser, reverbs, delay, chorus, flanger, phaser, ring mod, saturator, lo-fi, pitch shifter, dither, signal generator, three instruments). Each `ParamInfo` is `{id, name, min, max, default, unit: Db|Hz|Ms|Percent|Ratio|Semitones|Cents|Seconds|Toggle|Choice, taper: Linear|Log, choices}`.

- **NodeDef:** `sc.fx.<plugin>`. Input `audio: AUDIO`, output `audio: AUDIO`, plus one `FLOAT`/`BOOL`/`ENUM` port per `ParamInfo`. `Taper::Log` means a log slider, `unit` becomes the suffix, and `choices` become `ENUM`. Generators (`signal_generator`, the instruments) have no `audio` input but take `duration`/`MIDI`. Plus the built-ins `normalize`, `reverse`, `invert`, `gain`, `time_stretch`, `pitch_shift`, `varispeed` with their documented ranges.
- **Execution today (no source change):** run `new_session`, then `import_audio` from the input's file path. Select the clip, `execute audiosuite.process {process, params, clips:[id]}`, then `bounce_mix` to a write-root path. Convert the WAV to `.npy` (f32 planar `(C, N)`) in commonkit, and drop the session.
- **Execution after a small soundcraft change (proposed):** a pure `dsp.process {in: path, out: path, process, params}` command, plus `.npy` read/write in `soundcraft-audio-io`. The adapter then never builds a session, the arrays are mmap'd both ways, and the result is a function of `(input bytes, plugin id, clamped params, soundcraft version)`. This is the same shape as the research software's op nodes.

Equivalent catalogues elsewhere:

- vectorcraft's trace presets and pathfinder operations (`pathops::boolean`, `offset`): input `SVG`/`IMAGE`, output `SVG`;
- wordcraft's `convert`/`text`/`render`: input `DOC`, output `TEXT`/`IMAGE`/`PDF`;
- lightcraft's develop settings: input `IMAGE`, output `IMAGE`. This overlaps the research software.

### B. Command nodes with document threading (generic, any craft)

This works for any command whose `effect` is `Mutate`. The node has an input `doc: DOC<app>` and an output `doc: DOC<app>`. Its params come from `params_schema`: one widget per property when the schema exists, else a single JSON-text widget with the prose `params` as tooltip. The command's JSON result appears as a `result: JSON` output, so query results can feed parameters (`$N` references in vectorcraft's `run` already do this).

- **The DOC value is a CAS handle**, never bytes in a message. It is `{app, key: md5(saved document bytes), sources: [md5 …]}`. Referenced media, such as soundcraft's Audio Files folder or vectorcraft's placed images, are content-addressed separately, so a chain of edits does not re-upload media.
- **Selection becomes a parameter.** Commands whose `enabled` judges the selection (`has_selection`, `has_range`) get `ids`/`tracks`/`clips`/`start`/`end` ports, as lightcraft's `explicit_targets` and soundcraft's `clip_ids_param` already allow. A command that can only act on the UI selection is marked not node-safe.
- **Excluded:** `View`/`Ui` commands, undo/redo, app-only tools (`click`, `drag`, `menu_invoke`), and anything that opens dialogs.
- **Cache key:** `md5(app id, app version + build, command id, canonical params, input DOC key)`. This is a Merkle chain over the inputs, as in the research software's executor. A rerun hits the cache even when the saved bytes aren't byte-stable (timestamps, fresh ids in the file). Output keys are only used for dedupe, and only for apps whose save is canonical, which needs a test per app.

**Efficiency: fused segments.** Opening and saving a session for every node is too slow for a chain of 20 edits. So the planner (the same component as the research software's M4 planner) fuses a linear run of command nodes on one DOC lineage into a single `batch` call against one live session. It materialises (calls `save_session` and hashes) only at:

- branch points;
- nodes the user previews;
- checkpoint nodes.

The intermediate DOC keys are still computed from the Merkle chain, so a later edit upstream invalidates exactly the right nodes.

### C. Snapshot and export nodes (typed data out)

These are `Query`/`Io` commands that turn a DOC into data:

- `bounce_mix` gives `AUDIO`;
- `inspect_session` gives `JSON`;
- vectorcraft `export` gives `SVG`/`PDF`/`IMAGE`;
- wordcraft `text` gives `TEXT`, and `render` gives `IMAGE`;
- gridcraft ranges give `TABLE`.

They are the bridge from document engines into the generic wire types the other engines share (OpenCV, text and audio examples). For example: the research software's picture → vectorcraft `trace` → `SVG` → the research software layer, or an image → wordcraft document → PDF.

## Wire types this needs from the generic base

- `AUDIO`: a commonkit `.npy` `(C, N)` float32, with meta `{sample_rate, channels, frames}`.
- `DOC<app>`: an opaque CAS handle, displayed by its thumbnail or summary.
- `SVG`/`VECTOR`, `PDF`, `TEXT`, `JSON`, `TABLE`. A later `MESH` covers cad/solid.

The UI never decodes a DOC itself. It asks the engine for a display payload, so visualisation stays engine-driven, as the research software's `display.py` is today.

Display payloads (generalising the research software's 8-bit RGBA preview segment):

| Type | Preview the engine writes | Interactive in UI |
|---|---|---|
| AUDIO | peaks segment (soundcraft already computes `audio_io::peaks::Peaks`) + spectrum/LUFS/peak dBFS text; optional short PCM preview segment | waveform zoom, play preview through the UI's audio output, A/B between two nodes |
| SVG / DOC(vector) | PNG render at ≤640 px (`export … --scale`) + object counts | toggle outline view |
| DOC(word) / PDF | page PNG (`wordcraft-cli render --page N`) + page/word counts | page stepper |
| TEXT / JSON / TABLE | text, a JSON tree or a table, inline | scrolling and search |

## Status tracking and data in flight (no less than the research software today)

The research software's `StatusReducer` (`crates/bridge/src/status.rs`) consumes records keyed by `(run_id, node_id)`. The states are `queued, started, progress, preview, done, cached, error, skipped, cancelled`, with `fraction`, `outputs`, `display` and `error`. A craft knows nothing about graph nodes, so **the adapter is the executor** for MCP engines and emits those same records itself:

- `queued` when planned; `cached` on a CAS hit, with no MCP call;
- `started` before the `execute`/`batch` call;
- `progress` from `notifications/progress` when the craft sends them (vectorcraft, lightcraft, effectcraft, filmcraft). Otherwise the adapter reports per-step progress inside a fused batch (step k of n), which is the same granularity the research software gets between nodes;
- `preview` after each materialised node, with its display payload. This is how data in flight is visualised: inside a fused segment, previews are materialised for the nodes the user is watching;
- `done`/`error` from the JSON-RPC result or `isError`. Downstream nodes are `skipped`;
- `cancelled` via `notifications/cancelled`, which effectcraft and vectorcraft honour mid-render; otherwise the adapter kills and respawns the child, since sessions are disposable.

The records go out through whatever status channel the generic interface defines (callback in-process, a stream for separate processes). So the UI treats a craft engine exactly like the Python/pipekit engine.

## The adapter (crate `engine-mcp`, layer L2 beside `bridge`)

- **Transport:** spawn `<app>-cli mcp --headless --automation-read-root <cas> --automation-write-root <scratch>` as a stdio child. Use the **rmcp client** (`transport-child-process`); filmcraft already depends on rmcp, so the crafts' dependency set is in favour. rmcp gives spec-correct progress, cancel and protocol negotiation, and later the streamable-HTTP transport. If rmcp's tokio dependency clashes with `bridge`, a hand-rolled newline JSON-RPC client is about 400 lines, mirroring the crafts' own servers. Every reply is hostile input (golden rule 1).
- **Pool:** one document per server process, so parallel branches need N processes (bounded, LRU of warm sessions keyed by the DOC key they hold). A warm process that already holds the DOC a node needs avoids an `open_session`.
- **Schema generation:** at engine start, `list_commands` plus the catalogue queries (`engine.plugins`, …) plus `<app>.manifest.toml` produce NodeDefs, with `codegen: null`. The manifest says which commands are node-safe, their DOC/data ports and their families. Version is the `serverInfo.version`, so a craft upgrade invalidates the cache.
- **Remote:** run the same craft CLI on the remote host behind the generic remote engine (MCP over SSH stdio, or MCP streamable HTTP once a craft serves it). DOC and AUDIO handles are MD5 keys, so the client uploads only the keys the remote CAS lacks and downloads only previews plus the outputs the user asks for.
- **Data rule:** arrays and documents never travel in JSON-RPC bodies. MCP's `screenshot` base64 image path is not used for data, only for app-only diagnostics.

## Effort (on top of the FloCraft extraction)

| Item | Days |
|---|---|
| `engine-mcp` client, pool, status emission, cancel | 4-5 |
| Schema generator from `list_commands` + catalogue + manifest | 2 |
| SoundCraft pack: catalogue nodes, `bounce`/`import` glue, AUDIO display (peaks, spectrum, playback) | 4-5 |
| Source fixes in soundcraft: `dsp.process`, `.npy` I/O | 1-2 |
| `params_schema` + `effect` across craft-libs and the crafts (mechanical sweep, tests) | 3-4 |
| VectorCraft pack: trace/pathops/export nodes, SVG display | 3 |
| Fused-segment planning for DOC chains | 2-3 |

## Risks

- **Determinism.** A session that renders differently on a second run, from seeded noise (`lofi`, pink noise) or plugin state, breaks cache reuse. Seeds must become params; a test per pack checks that `run twice → same bytes`.
- **Selection-only commands.** Some commands can't be addressed without the UI selection, so they stay out of the palette until they take explicit targets.
- **Long documents.** Bounce limits (`MAX_OFFLINE_SAMPLES`, `render_limit`) surface as node errors, not panics.
- **Version skew.** A craft binary rebuilt underneath a cache gives stale hits unless the version string includes the build hash.
