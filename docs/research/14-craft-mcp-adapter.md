# 14: The craft MCP adapter, VectorCraft first (milestone M, step M1)

*2026-10-27. Research and design note for `docs/plan.md` 10.6 (D15). Design only: no PlumCraft source, nothing changed in VectorCraft or craft-libs. VectorCraft is at `~/src/artcraft/vectorcraft` (HEAD `8bf090b2`, version 0.16.0, release `vectorcraft-cli` built 2026-10-24). Probes, logs and outputs are in `claude-debug/craft-mcp/` (`mcp_probe.py`, `mcp_cancel_repro.py`, `classify.py`, `infer_ports.py`, `commands.json`, `classes.json`, `mcp_probe.log`). The test image is `test.jpg`, the corgi on a floral carpet (811 × 812 px, copied there as `corgi.jpg`, and upscaled to 3196 × 3200 as `corgi_big.jpg`).*

## Conclusion

- **VectorCraft's MCP server is a good substrate, better than note 05 said in some ways and worse in others.** Better: the `params` prose follows a grammar the engine itself parses to reject unknown keys (all 811 descriptions parse), and three typed catalogues exist (`imageTrace.presets`, `effect.list` with typed defaults for 116 effects, `document.formats` with typed options for 24 formats). The native save is byte-stable and open→save is the identity, object ids survive a save, and with automation roots the server runs hermetically, without the user's preferences or plug-ins. Startup is under 50 ms. Worse: no command metadata says what a command touches (`journal` exists but is not exposed), 97 of the 183 selection-gated commands take no `ids`, and progress and cancel exist only for exports, PDF, flattening and raster effects. Image Trace, pathops and recolor neither report progress nor stop when cancelled. A cancelled request gets no reply at all.
- **The adapter is an ordinary PlumCraft engine built on `plumcraft-engine`** (spec 16.5). It owns the walk and emits every state. It threads a document as a `doc` DataRef: an `md5:raw:` handle of the uncompressed native save, stored as a real `.vectorcraft` file in the adapter's own CAS, which is the craft's read root. Selection becomes a `select` parameter compiled into `select.*` calls. Cancel kills the child, since respawning costs about 50 ms. A craft crash fails that node, and the pool replaces the process. The memo uses plan keys salted with the craft binary's digest.
- **First nodes: 24 curated nodes plus 116 generated effect nodes**, with ports inferred from the prose grammar, the typed catalogues and a hand-trimmed manifest, and a generic `vc.run` escape hatch. The raster → Trace → Recolor → SVG chain on the corgi runs in about 0.4 s headless today (`vectorcraft-cli run`).
- **Estimate:** M2 is 15 days (13–17) and M3 is 6–7 days. Decisions D40–D50 are below. They touch deferred decisions D9–D11, D14 and D20, which are named where they apply.

## 1. VectorCraft as built

### 1.1 Processes and transport

| Item | As built | Where |
|---|---|---|
| Entry | `vectorcraft-cli mcp [--connect ADDR `\|` --headless] [--automation-read-root DIR]... [--automation-write-root DIR]...` | `apps/vectorcraft-cli/src/main.rs:71`, `mcp()` at 182 |
| Default backend | **Connects to a running desktop app at 127.0.0.1:7979 if one is listening**, else headless. The adapter must pass roots or `--headless`, or it could drive the maintainer's open app | `main.rs:220-227` |
| Roots | Imply headless. They confine every file the process reads or writes, and are installed process-wide (`file_access::confine_process`). **With roots, the profile, plug-in and PDF-preset stores are not attached** (`with_stores` is skipped), so defaults are the built-in ones | `main.rs:175-180, 205-218`; `crates/engine/src/file_access.rs:141` |
| Framing | Newline-delimited JSON-RPC 2.0, hand-written, no async runtime. Request lines are capped at 16 MiB, and old-style batches at 64 messages | `crates/mcp/src/lib.rs:1-13`; `server.rs:41-44` |
| MCP revisions | `2026-07-28`, `2025-06-18` (default), `2025-03-26`, `2024-11-05` | `server.rs:21-23` |
| `serverInfo` | `{"name": "vectorcraft", "title": "VectorCraft", "version": "0.16.0"}`: `CARGO_PKG_VERSION` only, **with no build hash** | `server.rs:421` |
| Capabilities | `tools, resources, prompts{listChanged:false}, completions, logging` | `server.rs:413-420` |
| Threads | A reader thread, the engine thread (calls run one at a time, in arrival order) and a writer thread. The reader answers `ping`, `tools/list`, `resources/list`, `resources/templates/list`, `prompts/list` and `prompts/get` itself, **but only once a call has run for `busy_after` (2 s)**. Before that they queue | `intake.rs:1-16`; `server.rs:114-133, 269-298` |
| Busy | A request that would wait behind a call older than 2 s is refused with `-32003` and `data.busy {requestId, running, seconds}` | `server.rs:32-36`; live: `D busy` in `mcp_cancel_repro.py` |
| Startup | `initialize` answered 4–46 ms after spawn (two runs). `commands` in 0.32 s wall time | `mcp_probe_summary.json` |
| Documents | **One process holds many documents** (`open_file` returns `index` 1, 2, 3 …). One is active; `document.activate {index}` and `file.close {index?}` manage them. Commands act on the active one. Note 05's "one document per server process" is wrong; the true limit is one *call* at a time | live probe |
| Panics | A panicking request fails that request with `-32603` "internal error … (please report this bug)"; the server keeps serving | `server.rs:386-388` |

### 1.2 MCP methods and tools

There are 32 tools. They are *meta* tools that dispatch by command id. Several are aliases with different argument names, which matters to a client:

| Tool | Arguments | Result (text content, JSON) | Notes |
|---|---|---|---|
| `command_list` | `filter?/query?, menu?, enabled_only?, brief?` | bare array of `CommandInfo` | `tools.rs:118`, dispatch 991 |
| `list_commands` | same, `enabledOnly` | `{count, commands}` | alias |
| `command_run` | `id, params?` | the command's JSON result | `tools.rs:1011` |
| `run_command` / `invoke_menu` | `command, params?` | same | aliases |
| `command_batch` | `steps[<=256] {id, params}`, `stop_on_error=true`, `atomic=false`, `label?` | `{completed, failed, results[{ok, result`\|`error}], created}` | `$N.path` and `$last.path` references; kept results ≤ 8 MiB (`tools.rs:138, 1218, 1232`) |
| `doc_inspect` / `inspect_document` | `depth, ids, kind, limit, offset…` | artboards, layer tree, selection, history, `dirty` | |
| `open_file` | `path` (absolute) | `{index, title, format, warnings, missingLinks…}` | `tools.rs:1087` → `app.open` → `document.open` |
| `save_file` | `path?, compress?, version?, preview?, options?` | `{path, format, bytes, warnings}` | |
| `export` | `format?, path?, scale?, artboard?, range?, selection?, outlineText?, options?` | `{path, format, bytes, warnings}`; **without `path`, `dataBase64`** | |
| `render_preview` | `max_side <=4096, artboard` | inline PNG image content | diagnostic only for us |
| `screenshot` | `path?, scale?, artboard?, window?` | PNG content | |
| `draw_path`, `draw_shape`, `set_paint`, `add_text`, `apply_effect`, `pathfinder`, `transform`, `create_graph`, `text_wrap`, `undo`, `redo` | typed JSON Schemas | convenience wrappers over commands | |
| `select_tool`, `pointer_gesture`, `press_key`, `type_text`, `open_panel`, `ui_inspect`, `inspect_ui`, `ui_screenshot` | | tool errors when headless | never nodes |

The other MCP methods are `resources/read` (`vectorcraft://document`, `…/document/json`, `…/commands`, plus the templates `vectorcraft://object/{id}`, `…/command/{id}`, `…/effect/{id}` and `…/swatch/{name}`), `prompts/*` (`poster`, `icon-set`, `recolor`, `trace-and-style`, `export-set`), `completion/complete` and `logging/setLevel` (`server.rs:403-460`).

**Tool annotations** carry a `readOnlyHint` per tool (`tools.rs:102-112`). It describes the *tool*, not the command, so `command_run` is always "not read-only". There is no per-command effect flag on the wire.

### 1.3 Commands: `CommandSpec`, `CommandInfo`, `enabled`

```rust
pub struct CommandSpec { id, label, menu: &[&str], shortcut: Option<&str>, params: &str,
                         enabled: fn(&Session) -> Result<(), String>, run: fn(&mut Session, &Value) -> Result<Value>,
                         journal: bool }                        // crates/engine/src/cmd/mod.rs:109-123
pub struct CommandInfo { id, label, menu, shortcut, params, enabled: bool, disabled_reason: Option<String> }  // :126-135
```

- **811 commands** in the catalogue (`vectorcraft-cli commands`, or `command_list`): 807 engine commands plus 4 headless host commands (`file.open/save/export/exportForScreens`, `headless.rs:34`). Note 05's count of 377 came from a regex.
- `journal` is the only side flag: false for queries and selection helpers, via the `cmd!(query …)` macro (`mod.rs:196-203`). **`CommandInfo` doesn't serialise it**, so over MCP no command says whether it mutates. SoundCraft's spec has `journal`, `undoable` and `view` (`soundcraft/crates/engine/src/cmd/mod.rs:71-85`), and VectorCraft has none of those. **craft-libs has no `CommandSpec` and no MCP code** (searched every checkout under `~/.cargo/git/checkouts/craft-libs-*`): the "shared model" is a convention each craft copies, not a type.
- `enabled` is evaluated against the live session. Probing it in three states (no document; a document with nothing selected; everything selected) is the most reliable signal there is. With a document open and nothing selected, 183 commands report `"nothing selected"`, 6 `"select at least two objects"`, 3 `"no anchor points selected"` and 5 `"no slice selected"` (`classify.py`).

### 1.4 The `params` format in practice

Every description starts with the params object, in a small grammar the engine parses itself (`crates/engine/src/cmd/params.rs:1-19`):

- `{key, key?: …, a?|b?: …, key=default, …}`. `?` marks an optional key, `|` gives alternatives, and `:`, `=` or `(` starts the prose about it. `{…} or {…}` gives alternative objects. `…as other.command` imports another command's keys.
- `→ {…}` describes the result.
- **Callers from outside (MCP, the CLI, the control channel) get a parameter error for any key the description doesn't name**, with the nearest key suggested (`check_keys` at `params.rs:217`, `check_params` at 462, applied in `headless.rs:129-135`). Effects are also checked for type against their defaults (`check_effect_params`).

Real examples, from `commands.json` (trimmed where marked …):

| # | Command | `params` (verbatim start) |
|---|---|---|
| 1 | `imageTrace.makeAndExpand` | `{…as imageTrace.make} same as imageTrace.make, then expand → {id, paths, anchors, colors}` |
| 2 | `imageTrace.make` | `{id?: image or Image Trace group (default: selection), preset?: name (imageTrace.presets), enhanced?: false …, params?: {mode: "blackAndWhite"`\|`"grayscale"`\|`"color"`\|`"logo", threshold: 0-255, palette: "limited"`\|`"fullTone"`\|`"automatic"`\|`"documentLibrary" …, colors: 2-256, colorDetail: 0-100, paths: 0-100, corners: 0-100, noise: px, method: "abutting"`\|`"overlapping", ignoreWhite, …}, view?, ids?} → {id, paths, anchors, colors, warnings}` |
| 3 | `imageTrace.presets` | `{} → {presets: [{name, params, enhanced}]}`, 15 presets: Default, Auto Color, High Fidelity Photo, Low Fidelity Photo, 3 Colors, 6 Colors, 16 Colors, Shades of Gray, Black and White Logo, Sketched Art, Silhouettes, Line Art, Technical Drawing, Outline, Flat Logo |
| 4 | `recolor.colors` | `{ids?: object ids (default: the selection, or with nothing selected the objects select.all takes …)} → {colors: [{key, hex, count, swatch?}]}` |
| 5 | `recolor.apply` | `{map: rows [{from: [colour keys … or "#rrggbb"], to: colour `\|` null, exclude?}] `\|` {"key or #rrggbb": colour, …}, method?: "exact" (default)`\|`"preserveTints"`\|`"scaleTints"`\|`"tintsShades"`\|`"hueShift", limitTo?, group?, groupColors?, rename?, includeImages?: true, includePatterns?: true} … → {changed, swatches, relinked?, note?}` |
| 6 | `recolor.reduce` | `{colors?: n (rows …), method?: as recolor.apply; default scaleTints, preserve?: {white?: true, black?: true, grays?: false}, limitTo?, ids?} …` |
| 7 | `recolor.randomize` | `{map: rows as recolor.apply, order?: true, saturationBrightness?: false, seed?: 0} → {map}` |
| 8 | `object.path.simplify` | `{tolerance?: pt (1), level?: 0..100 …, auto?: bool …, cornerAngle?: deg (30), straightLines?: bool, wholePaths?: bool …} → {ids, before, after}` (no `ids` key: selection only) |
| 9 | `object.path.offsetPath` | `{offset: pt (negative insets), joins?: "miter"`\|`"round"`\|`"bevel", miterLimit?: 4} → {ids}` |
| 10 | `object.pathfinder.unite` (and nine more) | `{ids?: [id…] (selected first; default: the selection)} operate on the selected objects (back → front) → {ids}` |
| 11 | `object.scale` | `{ids?, sx: %, sy?: %, origin?: [x,y], copy?, strokes?: bool, corners?: bool, objects?: bool, patterns?: bool, typeAreas?: bool} …` |
| 12 | `object.transform` | `{matrix: [a,b,c,d,e,f], copy?: bool, ids?, strokes?, corners?, typeAreas?, anchors?, objects?, patterns?} …` |
| 13 | `object.vectorHalftone` | `{shape?: "circle"`\|`"ellipse"`\|`"square"`\|`"diamond"`\|`"line" (circle), frequency?: lines per inch 1–300 (20), angle?: deg (45 …), mode?: "mono"`\|`"cmyk", color?, invert?, clip?: true, keepOriginal?: false} …` |
| 14 | `effect.apply` | `{effect: id (see effect.list …), params?: {…} (missing keys take the dialog defaults), item?, ids?`\|`id?, target?: "object"`\|`"contents"} … → {ids, index, item, grouped?}` |
| 15 | `document.export` | `{path?, format?: svg`\|`svgz`\|`pdf`\|`ai`\|`png`\|`jpg`\|`…`\|`psd`\|`vectorcraft`\|`template (default: from the path's extension, else png …), params?, …the SVG options given flat instead, …the PDF options of document.exportPdf …}` |
| 16 | `document.open` | `{path `\|` name `\|` dataBase64 (a file's path, or its name with its bytes), pages?, page?, cropTo?: bounding`\|`art`\|`crop (default)`\|`trim`\|`bleed`\|`media, …}` |
| 17 | `file.new` | `{preset?, name?`\|`title?, width?: pt=612`\|`"210 mm", height?: pt=792, units?, orientation?, …, colorMode?: "rgb"`\|`"cmyk", …, created?: Unix seconds`\|`null (… default now …)} → {index, previewMode}` |
| 18 | `select.set` | `{ids: [id…]} all ids must be non-negative integers naming existing objects → {count, ids}` |

**Typed catalogues** that need no prose parsing:

- `effect.list` returns 116 effects, each as `{id, label, menu, params (prose), defaults (typed JSON values), raster, lengths}`: 15 Artistic, 15 Warp, 14 Sketch, 13 Pathfinder, 8 Brush Strokes, 7 Distort & Transform, 7 Stylize, 6 Texture, 6 Color Adjustments, and so on.
- `document.formats` returns 24 formats. Each has `options: {name: {type, default, description}}`, for example SVG's `decimals` (integer, 3), `embedFonts` (boolean) and `encoding`, and PNG's `antiAlias, artboard, background, ppi, scale, …`.
- `imageTrace.presets` gives 15 full parameter sets with typed values (`threshold: 128`, `paths: 50.0`, `mode: "blackAndWhite"` …).

### 1.5 `commands`, `describe`, `info`, `run`, `convert`

- `vectorcraft-cli commands` prints the catalogue as a JSON array of `CommandInfo` evaluated with no document (so most commands say `enabled: false, "no document open"`).
- **There is no `describe` subcommand** ("unknown subcommand `describe`"). Per-command detail is `vectorcraft://command/{id}` over MCP, or a filtered `command_list`. Note 05 credited `describe` to VectorCraft; it is SoundCraft's.
- `info FILE` prints import warnings, title, colour mode, units, artboards, object counts by kind and fonts (`corgi.jpg`: one 811 × 812 artboard, 2 objects).
- `run [--in F] [--cmd ID [--params JSON] | --commands FILE|-]... [--export F]... [--scale N]` prints one JSON line per step, and `$N` references work across steps. The corgi chain `--in corgi.jpg --cmd select.all --cmd imageTrace.makeAndExpand --params '{"preset":"6 Colors"}' --export trace6.svg --export trace6.png --export trace6.vectorcraft` took **0.41 s** (1097 paths, 31,385 anchors, 6 colours; SVG 1.27 MB, native 3.7 MB). A recolor of the native file with a 6-row map, exported to SVG and PNG, took 0.07 s.
- `convert IN OUT` opens and exports in one step.

### 1.6 Document lifecycle, selection, identity

| Behaviour | Observed | Consequence for the adapter |
|---|---|---|
| Open a raster | `document.open {path}` gives a document whose artboard is the image's size, with the image as an object | Trace's input is "open the IMAGE, then `select.all`" |
| Native save stability | Two runs of trace and export: identical `.vectorcraft`, SVG and PNG bytes. `simplify` twice: identical. **Correction (M2):** only within the same second. Every save stamps File Info's `modified` (and `created` when empty) with the clock unless `SOURCE_DATE_EPOCH` is set (`crates/doc/src/metadata.rs` `output_time`, `cmd/fileio/save.rs` `stamp_save_dates`); two saves a second apart differ | A content key on docs is sound **only with `SOURCE_DATE_EPOCH` pinned** in the craft's environment (the M2 pack sets it to 0) |
| Open → save | Byte-identical to the file opened (`trace6.vectorcraft`, `big.vectorcraft` at 218 MB). **Correction (M2):** with `SOURCE_DATE_EPOCH` pinned; without it `modified` moves on | Re-materialising a doc never changes its handle |
| `file.new` | Writes `"created": <now>` into File Info, so two new documents differ | The `vc.new` node passes `created: null` |
| PDF | `/ModDate` changes per run | PDF outputs have a plan key only, never a content key |
| Compressed native save | `compress: true`: 77 MB instead of 218 MB, 2.66 s instead of 0.79 s, **and not byte-stable** (the gzip header) | Never compress in the craft; the store may compress |
| Object ids | Persist through save and reopen (`select.set {ids:[1415]}` after reopen selects the traced group) | Ids from one node's result can target a later node |
| Selection | **Not saved** in the native file; empty after open | Selection must be an explicit parameter (section 3.3) |
| Size and time | HFP trace of `corgi_big.jpg`: 2.99 s (10.4 s CPU), 46,140 paths, 1.85 M anchors, native **218 MB**. Open 0.47 s, open+save 0.79 s, MD5 0.27 s, SVG export 1.34 s (78 MB) | Big documents make per-node save measurable; see D42 |
| In-band bytes | `document.open {name, dataBase64}`, `export` without `path` and `document.serialize` return or take bytes in JSON (the SVG came back as 2.2 MB inline) | Not used: data never travels in JSON-RPC bodies, and the 16 MiB line cap would break |

### 1.7 Progress, cancellation, errors

- **Progress:** sent only when the request carries `_meta.progressToken`. Notifications are throttled to one per 200 ms and dropped if the client reads slowly (`server.rs:49-78`). Emitters: raster export ("Rendering artboard 1 of 1", `progress 0, total 1`), PDF pages, Export for Screens, transparency flattening, raster effects and plug-ins (`grep cancel::progress`). **Image Trace, pathops, recolor and simplify emit nothing.** Live, a 6.4 Mpx PNG export sent one notification, and a 3 s trace sent none.
- **Cancel:** `notifications/cancelled {requestId}` drops a queued request or raises the running one's token, cooperative, per thread (`crates/doc/src/cancel.rs:1-12, 74, 91`). **The cancelled request gets no reply, ever** (`server.rs:316-328`). Live: a PNG export at scale 4 stopped and wrote no file. A High Fidelity Photo trace **ignored the cancel, ran to the end and changed the document** (history "Image Trace (Make and Expand)", `dirty: true`), and its reply was suppressed. A cancel naming a finished or unknown request is harmless (cases A and B).
- **Errors** are tool results with `isError: true` and a text message, not JSON-RPC errors:

| Case | Message (live) | Maps to (section 3.8) |
|---|---|---|
| Unknown key | `command_run: invalid parameters for \`object.path.simplify\`: unknown parameter \`levl\`; did you mean \`level\`?` | `config` |
| Not enabled | `command_run: command \`object.path.simplify\` is not available right now: nothing selected` | `config` (names the `select` port) |
| Unknown command | `command_run: unknown command \`no.such.command\ | `config` (catalogue skew; triggers `schema.changed`) |
| Wrong target | `invalid parameters for \`imageTrace.makeAndExpand\`: select an image (or an Image Trace group)` | `config` |
| Outside a root | `open_file: automation path rejected: outside the read root …: /etc/hosts` | `engine` (adapter bug) |
| Busy | JSON-RPC error `-32003`, `data.busy {requestId, running, seconds}` | adapter bug (it never queues) |
| Panic | JSON-RPC `-32603` "internal error in … (please report this bug)" | `op` |

- **One unexplained stall:** in the first probe run, after a late cancel, two `doc_inspect` calls on the 1.85 M-anchor document got no reply within 120 s. Eight targeted repros (late cancel, unknown cancel, cancel mid-trace, busy, repeated inspects) did not reproduce it. The adapter's per-call timeout and kill cover it either way; it is recorded as a risk (R6), not a finding.

### 1.8 Concurrency

There is one engine thread per process, so calls are serial. A long call makes later requests queue, and after 2 s they are refused as busy. Parallel graph branches therefore need several processes. Image Trace is already multithreaded inside (10.4 s CPU over 3 s wall time), so more than about four processes gain little on the development machine.

### 1.9 How a second craft differs (SoundCraft)

SoundCraft's server (`soundcraft/crates/automation/src/mcp.rs`, HEAD `aa74bff`) is a single loop, `for line in input.lines()` (`mcp.rs:250-262`). It has no reader thread, so it has no cancel, no ping while busy and no progress. Its tools are `list_commands`, `execute {command, params}`, `batch {calls[<=1000], keep_going}`, `inspect_session`, `new_session/open_session/save_session`, `import_audio` and `bounce_mix` (`mcp.rs:126-195`), and it takes the same `--automation-read-root/--automation-write-root` flags. So the shape is the same and the names differ: a manifest maps them (section 3.10). Of the crafts, only filmcraft and photocraft depend on `rmcp` (3.5, server side). VectorCraft and SoundCraft are hand-rolled.

### 1.10 Where note 05 was wrong or out of date

| Note 05 said | As built |
|---|---|
| 377 commands | 811 (807 engine + 4 host) |
| Tools `list_commands`, `execute`, `batch {calls <=1000, keep_going}`, `inspect_session`, `new_session` | Those are SoundCraft's. VectorCraft has `command_list`/`list_commands`, `command_run`/`run_command`, `command_batch {steps <=256, stop_on_error, atomic}` with `$N`, `doc_inspect`, `open_file`, `save_file`, `export` |
| One document per server process | Many documents per process; one call at a time |
| Progress and cancel implemented in VectorCraft | Only for exports, PDF, flattening, raster effects and plug-ins; trace and pathops ignore cancel |
| `params` is unstructured prose; a JSON Schema is needed for ports | A parsed grammar with unknown-key rejection, plus three typed catalogues; a schema would still help types (60–70 % inferred, section 2.4) |
| Flags `journal`/`undoable`/`view`/`mutates` per craft are the raw material | VectorCraft has only `journal`, and it is not on the wire |
| A shared `CommandSpec` in craft-libs | No such type or crate; each craft copies the convention |
| Use the rmcp client; filmcraft depends on rmcp | Possible, but it brings tokio, which PlumCraft has none of, and the servers are hand-rolled JSON lines (D40) |
| VectorCraft `describe` CLI | No such subcommand |

## 2. Which commands can be nodes

### 2.1 How classes are inferred (no schema, no effect flag)

1. **`enabled` in three states** (`classify.py`): a command disabled with `"nothing selected"` in a document with no selection and enabled once everything is selected acts on the selection. This is exact, because it calls the command's own predicate.
2. **The params grammar:** top-level keys (`ids`/`id` = explicit targets; `path`, `folder`, `dataBase64`, `files` = I/O), whether a `→` result exists, and the prose after each key for types (section 2.4).
3. **Ids and menus:** `view.*`, `tool.*`, `help.*`, `prefs.*`, Window-menu panel commands = UI. `select.*` = selection. `file.*`, `document.export*`, `print.*`, `library.*`, `links.*` = I/O. Queries are recognised by an id tail of `list`, `info`, `inspect`, `get`, `presets`, `settings`, `formats`, `colors`, `node`, `find` or `json`.
4. **The tools' JSON Schemas**, where a convenience tool wraps a command (`pathfinder.operation` enum, `transform` numbers).
5. **A live smoke test:** run each candidate with its default parameters on a fixture document. A parameter or enablement error disproves the inference.

Signal 1 is exact; signal 2 is exact for keys and heuristic for types; signal 3 is right in most cases but misfiles some (`stroke.set` comes out as UI because it sits under Window › Stroke; `recolor.apply` is not selection-gated, because with nothing selected it takes what `select.all` would). So **classification seeds a manifest, and a person trims it**. Nothing is exposed only because a heuristic said so.

### 2.2 Classification of all 811 (first pass, `classes.json`)

| Class | Count | Node? | Examples |
|---|---|---|---|
| Art transform, takes `ids`/`id` | 191 | yes, with `select` | `object.scale`, `object.pathfinder.*`, `object.group`, `recolor.reduce`, `edit.clear` |
| Art transform, selection only (no `ids`) | 41 (97 when counted by `enabled`) | yes, with `select` compiled to `select.set` | `object.path.simplify`, `object.path.offsetPath`, `object.path.outlineStroke`, `object.rasterize`, `object.createObjectMosaic`, `type.createOutlines` |
| Other edits (not selection-gated) | 120 | some | `recolor.apply`, `recolor.randomize`, `object.distribute`, `edit.paste*` (clipboard: no) |
| Create | 18 | yes (generators with doc in and out) | `shape.*`, `path.create`, `text.create`, `graph.create` |
| Document-level (styles, swatches, layers, artboards, symbols, variables) | 169 | a few | `artboard.fitToArt`, `layer.flatten`, `swatch.addUsedColors` |
| Query (result only, no change) | 55 | yes, as JSON outputs | `recolor.colors`, `document.inspect`, `document.find`, `effect.list`, `object.blend.info` |
| Selection | 52 | no (they compile the `select` parameter) | `select.all`, `select.set`, `select.color`, `select.same.fillColor` |
| I/O | 114 | only through the adapter's own open and export nodes | `document.open`, `document.export`, `file.place`, `file.package`, `print.*` |
| UI and view | 37 | never | `view.*`, `help.*`, `edit.undo/redo`, `imageTrace.setView` |
| Gestures and tool-driven | 14 | never | `path.knife`, `path.freehand`, `object.liquify`, `object.puppetWarp`, `paint.sampleColor` |

**Reliability.** Keys are exact: the engine rejects any key its grammar doesn't name, so a generated port the craft refuses is caught by the smoke test. Selection dependence is exact from `enabled`. The I/O and UI classes are about 90 % right by name (spot-checked); the "other edits" bucket needs a person. **Purity** (deterministic given document bytes and params) cannot be inferred at all. Checked by running twice: trace, simplify, recolor, reduce, pathfinder and export to SVG/PNG/native are deterministic; `file.new` (time) and PDF (`ModDate`) are not. `recolor.randomize` defaults to `seed: 0`, so it is deterministic. The manifest records purity per node, and a conformance case runs every node twice (section 6).

### 2.3 The first nodes (24 curated, plus generated effects)

Port types: `vectorcraft.doc` (a `doc` DataRef), `IMAGE` (an encoded raster: PNG or JPEG), `SVG` (`vector`), `PDF` (`bytes`, media `application/pdf`), `JSON` (`tree`), plus the scalar types. Every node that changes art also has `select: STRING` (advanced, default `all`) and an optional `ids: JSON` socket (section 3.3). Every doc-out node also outputs `result: JSON`, the command's reply.

| Node | Command(s) | Inputs and widgets (inferred; source) | Outputs |
|---|---|---|---|
| `vc.open` | `document.open` | `source: IMAGE`\|`SVG`\|`PDF`\|`vectorcraft.doc` or `file: STRING` (path widget); `cropTo: ENUM` (prose); `pages: STRING` | `doc` |
| `vc.new` | `file.new` (`created: null` forced) | `width, height: FLOAT pt` (612, 792 from `=`), `units: ENUM` (10 quoted choices), `colorMode: ENUM rgb`\|`cmyk`, `artboards: INT 1–1000` | `doc` |
| `vc.trace` | `select.all`, `imageTrace.makeAndExpand` | `doc` (or `IMAGE`, auto-opened); `preset: ENUM` (15, from `imageTrace.presets`); `enhanced: BOOL`; advanced overrides typed from the preset values and ranged from the prose: `mode ENUM, threshold INT 0–255, palette ENUM, colors INT 2–256, colorDetail FLOAT 0–100, paths FLOAT 0–100, corners FLOAT 0–100, noise INT px, method ENUM, ignoreWhite BOOL, snapCurvesToLines BOOL, shapes BOOL, autoGrouping BOOL, gradients BOOL, fills BOOL, strokes BOOL, strokeWidth FLOAT px` | `doc`, `result {id, paths, anchors, colors, warnings}` |
| `vc.colors` | `recolor.colors` (query) | `doc`, `select` | `colors: JSON` (`[{key, hex, count, swatch?}]`) |
| `vc.recolor` | `recolor.apply` | `doc`, `map: JSON` (also a socket, so `vc.colors` can feed it), `method: ENUM` (5 choices), `limitTo: STRING`, `includeImages, includePatterns: BOOL` (true) | `doc`, `result` |
| `vc.reduce_colors` | `recolor.reduce` | `doc`, `colors: INT`, `method: ENUM`, `preserve: JSON` | `doc`, `map: JSON` |
| `vc.randomize_colors` | `recolor.randomize` | `doc`, `map: JSON`, `order: BOOL`, `saturationBrightness: BOOL`, `seed: INT` (0) | `doc`, `map: JSON` |
| `vc.simplify` | `object.path.simplify` | `doc`, `select`; `level: FLOAT 0–100` or `tolerance: FLOAT pt (1)`, `auto: BOOL`, `cornerAngle: FLOAT deg (30)`, `straightLines, wholePaths: BOOL` | `doc`, `result {before, after}` |
| `vc.offset_path` | `object.path.offsetPath` | `doc`, `select`, `offset: FLOAT pt`, `joins: ENUM miter`\|`round`\|`bevel`, `miterLimit: FLOAT (4)` | `doc` |
| `vc.outline_stroke` | `object.path.outlineStroke` | `doc`, `select` | `doc` |
| `vc.pathfinder` | `object.pathfinder.<op>` (10 commands, one node) | `doc`, `select`, `operation: ENUM` (from the `pathfinder` tool's schema) | `doc`, `result {ids}` |
| `vc.cleanup` | `object.path.cleanUp` | `doc`, `strayPoints, unpaintedObjects, emptyTextPaths: BOOL` (true) | `doc`, `result {removed}` |
| `vc.move` | `object.move` | `doc`, `select`, `dx, dy: FLOAT pt`, `copy: BOOL` | `doc` |
| `vc.scale` | `object.scale` | `doc`, `select`, `sx, sy: FLOAT %`, `origin: JSON [x,y]`, `copy, strokes, corners: BOOL` | `doc` |
| `vc.rotate` | `object.rotate` | `doc`, `select`, `angle: FLOAT deg`, `origin: JSON`, `copy: BOOL` | `doc` |
| `vc.fill` | `paint.setFill` | `doc`, `select`, `color: STRING` (colour hint), `none: BOOL` | `doc` |
| `vc.stroke` | `stroke.set` | `doc`, `select`, `weight: FLOAT pt`, `cap, join, align: ENUM`, `dash: JSON` | `doc` |
| `vc.expand` | `object.expand` | `doc`, `select`, `object, fill, stroke: BOOL` (true), `gradient: ENUM`, `steps: INT 1–1000 (255)` | `doc` |
| `vc.halftone` | `object.vectorHalftone` | `doc`, `select`, `shape: ENUM` (5), `frequency: FLOAT 1–300 (20)`, `angle: FLOAT deg (45)`, `mode: ENUM mono`\|`cmyk`, `color: STRING`, `invert, clip, keepOriginal: BOOL` | `doc` |
| `vc.mosaic` | `object.createObjectMosaic` | `doc` (an image selected), `columns, rows: INT 1–1000 (10)`, `spacingX, spacingY: FLOAT pt`, `gray: BOOL` | `doc` |
| `vc.fit_artboard` | `artboard.fitToArt` | `doc`, `index: INT` | `doc` |
| `vc.export_svg` | `document.export {format: svg}` | `doc`, the SVG options from `document.formats` (`decimals INT (3)`, `embedFonts BOOL`, `outlineText BOOL`, `artboard INT` …) | `svg: SVG` |
| `vc.export_png` | `document.export {format: png}` | `doc`, `scale: FLOAT (1)`, `ppi`, `background`, `antiAlias`, `artboard` (from `document.formats`) | `image: IMAGE` |
| `vc.export_pdf` | `document.exportPdf` | `doc`, `preset: STRING`, `range: STRING` | `pdf: PDF` (memo plan key only) |

**Generated, not curated:**

- **`vc.fx.<effect>`**: 116 effect nodes from `effect.list`, one widget per `defaults` key, typed from the default's JSON type, ranged from the effect's `params` prose and grouped by its `menu`. Each runs `effect.apply {effect, params, ids}`. One more node, `vc.expand_appearance` (`effect.expandAppearance`), bakes them. This is the craft's equivalent of SoundCraft's typed plug-in catalogue.
- **`vc.run`**: the escape hatch. A `command: ENUM` of every manifest-allowed id (transforms and queries, never UI, gestures or I/O), `params: JSON` with the prose as tooltip, and `select`. Its output is `doc` and `result`. It is pure only when the manifest marks the command so; otherwise its memo scope is `none`.

### 2.4 Port inference

1. **Keys** come from a Rust port of `doc_keys`/`accepted` (`params.rs:150-215`): the same grammar, so the same keys the craft accepts. The Python approximation in `classify.py` parsed all 811.
2. **Types** come from the text after the key (`infer_ports.py`, over all 2,504 top-level entries): `bool`/`true`/`false` → BOOL (17 %); a unit or numeric range (`pt`, `deg`, `%`, `px`, `ppi`, `n`, `0..100`, `0-255`, `1–1000`) → FLOAT or INT with `min`/`max` (12 %); quoted alternatives `"a"|"b"` → ENUM with `choices` (10 %); colours → STRING with a colour hint (1 %); `[…]`/`{…}` → JSON (8 %); `ids`/`id` → targets, not a widget (12 %); `path`/`folder`/`dataBase64` → I/O, owned by the adapter (2 %). That leaves 22 % named only (`index?`, `depth?`), 11 % prose-only and 1 % `…as other` imports. **About two thirds of all keys get a type mechanically.** Defaults come from `=x`, `(x)` and `(default x)`.
3. **Typed catalogues override prose** where they exist: trace presets, effect defaults and format options.
4. **The manifest overrides everything** for the curated nodes: renames, families, which keys are widgets and which are sockets, outputs lifted from the result (`before/after`, `map`), purity and memo scope.
5. **A smoke test gates the catalogue.** Each generated node runs with defaults on a fixture (the traced corgi). A `config` error fails the build of that NodeDef (the node is dropped with a log line), never a run.

The proposed craft-libs `params_schema` (deferred by D15) would turn step 2 into a lookup; nothing else in the design depends on it.

## 3. The adapter's design

### 3.1 Shape

`plumcraft-craft` is an engine crate built on `plumcraft-engine` (`Engine`, `Node`, `Cx`, the executor and memo in `exec.rs`, the host in `host.rs`). It is generic over crafts and reads one manifest per craft (section 3.10). A binary, `plumcraft-craft-engine --craft <manifest>`, serves it in mode 2 (D49). The craft is invisible to the UI: the UI sees NodeDefs, records and DataRefs, as with the text and audio engines.

```text
UI --protocol 2 (socket / HTTP / in-process)-- plumcraft-craft (Engine + executor + memo)
                                                 +- catalogue: manifest + command_list + typed catalogues → NodeDefs
                                                 +- craft CAS: <cache>/plumcraft/craft/vectorcraft/cas/ (raw files, real extensions)
                                                 `- pool: N × `vectorcraft-cli mcp --automation-read-root CAS --automation-write-root WORK`
                                                          (newline JSON-RPC over the child's stdin/stdout; stderr → 64 KiB ring)
```

### 3.2 The document handle

- **Value:** a `doc` DataRef. The handle is `md5:raw:<digest of the uncompressed native .vectorcraft bytes>`. `meta = {media: "application/vnd.vectorcraft+json", size, form: Doc{app: "vectorcraft"}}`, and `at = [Path{path: <cas>/<d[:2]>/<d>.vectorcraft, offset: 0}]` under the engine's declared work root (spec 4.4, which allows a work root).
- **The craft CAS** holds raw files with real extensions (`.vectorcraft`, `.svg`, `.png`, `.jpg`, `.pdf`), because the craft opens them by path and cannot read commonkit's `.npy` wrappers. Writes go to `WORK/tmp/`, are hashed by the adapter, then `rename`d into the CAS. The CAS is the read root; `WORK` is the write root; nothing else is reachable.
- **Inputs from PlumCraft:** an upstream `IMAGE`/`SVG` blob or a source file is staged into the craft CAS once, by its `raw` handle. The payload is read from its commonkit wrapper (`Path.offset`), or `clonefile`d on APFS from a source path, and named `<digest>.<ext>` from `meta.media` or the source name. Leaves are keyed by content (spec 14.3), as the SDK's `materialize_source` already does.
- **Outputs to the UI:** SVG, PNG and PDF are published to commonkit (`plumraw`, through `plumcraft-store`), so the UI resolves them as any other blob, and also stay in the craft CAS for later nodes. Docs are never published to commonkit: the UI never decodes a doc (spec 10.1), and a 218 MB doc exceeds the store's 128 MiB read cap (`MAX_READ`).
- **Cost per node** (measured): open 2 ms (811 px image) to 0.47 s (218 MB doc); command 0.1 ms (select) to 3 s (HFP trace); save 20 ms (3.7 MB) to 0.8 s (218 MB, open+save); MD5 10 ms to 0.27 s. The JSON-RPC round trip is under 1 ms. So the cost is materialisation, not MCP, and batching calls into `command_batch` saves almost nothing.

### 3.3 Selection as an explicit parameter

Every art node has `select: STRING` (default `all`) and an optional `ids: JSON` socket. The adapter compiles them before the command:

| `select` | Compiles to |
|---|---|
| `all` | `select.all` |
| `none` | `select.none` (for commands such as `artboard.fitToArt`) |
| `ids:1415,1416` or an `ids` wire (a JSON list, such as `vc.pathfinder`'s `result.ids`) | `select.set {ids}`, or `params.ids` directly when the command names `ids` |
| `color:#514a3d` | `select.color {fill: "#514a3d"}` |
| `kind:Path`, `name:…`, `json:{…}` | `document.find {…}`, then `select.set` with its ids |

- The selection is reset at the start of every node, so a warm document's leftover selection never leaks.
- An empty selection is a node `config` error that names the `select` port, not the craft's "nothing selected".
- `select` is part of the memo key like any widget.
- No selection-producing nodes in v1 (D44).

### 3.4 Executing a node, and fused runs

1. **Lease a process.** Prefer the process whose active document is the input handle (warm), else any idle one.
2. **Cold path:** `document.open {path: <cas path of the input>}`. **Warm path:** none; the process still holds the document the previous node saved, and open→save is the identity (section 1.6), so this is the same value.
3. Compile `select` (section 3.3), then call `command_run {id, params}` with a `progressToken`.
4. **Materialise:** `document.save {path: WORK/tmp/x.vectorcraft}` (uncompressed), MD5, rename into the CAS, then emit `preview` and `done`. Export nodes call `document.export {path, format, …}` instead, and query nodes return their JSON with no save.
5. Every 20 nodes on a warm document, or above a history-memory threshold, call `edit.clearHistory`, so the undo stack doesn't grow without bound.

**Fusing (D42).**

- In v1, every node materialises. A chain on one document runs in one warm process, so it pays one open and *n* saves, not *n* opens and *n* saves.
- That keeps exact per-node states, per-node memo entries and per-node previews without any planner, and the `run.get` snapshot always has real handles.
- The 218 MB case is the cost: about 1 s per node.
- **Lazy materialisation** is the second step, after a measurement in M2: skip the save for an intermediate that has one consumer, is not watched, is not a target and is not `memo.persist`. Its downstream identity is the plan key alone, which spec 14.2 already supports.
- Lazy materialisation needs one SDK addition: `Cx::demand()`, saying whether an output has other consumers, is watched or is a target.
- `command_batch` is used only for the select-plus-command pair, and only when the round trip matters (it doesn't on stdio).

### 3.5 Node states (all emitted by the adapter)

| State | When |
|---|---|
| `queued` | at plan time (`status.queued`) |
| `cached` | a plan-key hit; no MCP call; `preview` with the stored outputs (as `exec.rs:188-200` today) |
| `started` | before step 1 |
| `progress` | from `notifications/progress` (`progress/total`, `message` → `stage`), rate-limited by `Cx::progress`. Where the craft sends none (trace, pathops), the adapter reports its own phases: "opening" (0.05), "running imageTrace.makeAndExpand" (0.1), "saving" (0.9), so the bar moves and the stage says what is slow |
| `preview` | the moment the node's outputs are in the CAS: a thumbnail (section 4) and the watched displays |
| `done` | with `ms` |
| `error` | section 3.8; downstream nodes `skipped` |
| `cancelled` | section 3.6 |

### 3.6 Progress and cancel

- **Cancel = kill (D41).** On `run.cancel`, the adapter sends `notifications/cancelled` to the busy child (useful for the calls that honour it), marks the node `cancelled` at once, and **SIGKILLs the child after 200 ms** unless a reply has arrived. Then the pool respawns it (under 50 ms).
- Waiting is pointless. The craft never answers a cancelled request, so the adapter cannot tell "stopped" from "still tracing", and a trace ignores the cancel anyway, then changes the document.
- The killed process's document state is discarded, which is free because documents live in the CAS.
- Unreached nodes get `cancelled`, inside the 500 ms the conformance suite allows (spec 19 case 6).

### 3.7 The pool, timeouts, crashes

- **Pool.**
  - Size: `min(4, cores / 2)` by default (manifest `pool.max`); one process is spawned at engine start and the rest on demand.
  - Every process runs `--automation-read-root CAS --automation-write-root WORK` (hermetic, headless), in its own process group, killed with the engine; stderr goes to a 64 KiB ring per process, shown on errors.
  - Processes idle for more than 60 s are retired, down to one.
  - An LRU keyed by the handle of each process's active document picks warm processes.
  - Parallel branches run in parallel processes. This needs the SDK executor to run independent nodes concurrently; today it is serial, so v1 uses one process per run and the pool serves concurrent runs and sessions (D49).
- **Liveness:** between calls, a `ping` with a 2 s timeout before reuse; a process that fails it is killed.
- **Timeouts:** each call has a manifest default (10 min) and a per-node override (`timeout_s`). On timeout the node fails with `engine` ("vectorcraft did not answer `imageTrace.makeAndExpand` in 600 s; the process was restarted") and the process is killed. Progress notifications extend nothing; only the reply ends a call.
- **Crash:** EOF or exit on the child fails *that node* with kind `engine`, carrying the stderr tail and the signal. The pool replaces the process, downstream nodes are `skipped`, and independent branches continue (D46). Five crashes in 60 s stop the pool and emit `engine.state unavailable{reason, hint}`, the hint coming from the manifest ("build VectorCraft: `cargo build --release -p vectorcraft-cli` in …").
- **Hostile input:** every reply is parsed with bounds. Reply lines are capped at 64 MiB; an overlong line kills the process. An unknown notification is skipped. The parsers join `crates/proto/tests/hostile.rs`.

### 3.8 Errors

| Craft says | Node error |
|---|---|
| `isError` with "invalid parameters", "unknown parameter", "is not available right now", "select an image" | `config`, message kept (≤ 180 characters shown), naming our port where we can map the key |
| `isError`, anything else | `op` |
| JSON-RPC `-32603` (a caught panic) | `op`, "VectorCraft bug: …" |
| `unknown command` | `config`, and the adapter refreshes the catalogue (`schema.changed`) |
| EOF, exit, timeout, overlong line, a root rejection | `engine` (D46) |

### 3.9 Memo

- **Plan keys only, as the SDK does today** (`exec.rs:177-178`): kind, node version, `engine_salt`, params coerced by port type, and upstream identities.
- **Salt (D47):** `"craft:vectorcraft:" + serverInfo.version + ":" + md5(binary)`. `serverInfo.version` has no build hash, so the adapter hashes the executable once per `(path, size, mtime)` (100 MB, about 0.13 s). A rebuild misses every record, which is what spec 14.2 asks for.
- **Scopes:**
  - `machine` for nodes the manifest marks deterministic (verified by the run-twice case).
  - `none` for `vc.run` on unmarked commands, `vc.export_pdf` content, and anything that reads time, system fonts or the clipboard.
  - Text nodes depend on installed fonts, so `machine` is right and `global` is never offered.
- **Content keys:** docs, SVG and PNG are byte-stable, so a `doc` output may also take a content key (`memo.content: true` on the trace, the expensive node). PDF never does.
- **Persistence:** follows D10 (deferred). The doc sizes make D11's 20 GB durable-tier budget a real constraint (one HFP corgi trace is 218 MB), so persisted doc records should be opt-in per node.

### 3.10 A second craft with only a manifest

```toml
id = "vectorcraft"
name = "VectorCraft"
command = ["~/src/artcraft/vectorcraft/target/release/vectorcraft-cli", "mcp"]
roots = { read = "--automation-read-root", write = "--automation-write-root" }
hint = "cargo build --release -p vectorcraft-cli in ~/src/artcraft/vectorcraft"
pool = { max = 4, idle_s = 60 }
tools = { list = "command_list", run = { tool = "command_run", id = "id", params = "params" },
          open = { command = "document.open" }, save = { command = "document.save" },
          export = { command = "document.export" }, select_all = "select.all", select_ids = "select.set" }
doc = { wire = "vectorcraft.doc", ext = "vectorcraft", media = "application/vnd.vectorcraft+json" }
catalogues = [ { command = "effect.list", path = "catalog", node_prefix = "vc.fx.", run = "effect.apply" },
               { command = "imageTrace.presets", path = "presets", into = "vc.trace" },
               { command = "document.formats", path = "formats", into = ["vc.export_svg", "vc.export_png"] } ]
cancel = "kill"            # or "cooperative" for a craft whose every long call honours cancel

[[node]]
id = "vc.simplify"; command = "object.path.simplify"; family = "Path"; select = "selection"
pure = true; outputs = { result = ["before", "after"] }
widgets = { level = { min = 0, max = 100, default = 50 } }
# … one block per curated node; generated nodes need none

```

**SoundCraft would need:**

- `tools.run = { tool = "execute", id = "command" }`, `list = "list_commands"`, and `open`/`save` mapped to `session.open`/`session.save`;
- `doc = { wire = "soundcraft.session", ext = "scraft" }`;
- a catalogue entry for `engine.plugins`;
- `cancel = "kill"`, because its server has no reader thread.

Its curated nodes would import an `AUDIO` value (`file.import_audio`) and bounce one (`file.bounce_mix`). The client, pool, CAS, selection compiler, memo and status code are unchanged. So "generic" means generic over this manifest, not over a shared Rust type (craft-libs has none). M2 proves it with a SoundCraft manifest that only lists its commands and runs one (section 6).

### 3.10.2 A pack's labels for its ports (B18, #59, 2026-11-10)

A pack may carry `[labels]`: a port name → the label shown for it, or a node id → a table of them (`plumcraft_proto::labels`). `build` puts them on the pack's own ports as their `label`, so they reach every editor as the engine's labels; an entry that doesn't read is a build warning (`labels`), never fatal. The editor's application table and the user's `labels.toml` go over them. PhotoCraft's is the first: `scaleStyles` → "Scale styles".

### 3.10.1 Packs versioned against their app (B17, #56, 2026-11-06)

The maintainer: "Packs should be versioned and selected against the engine (application) version."

- **Two fields at a pack's top:**
  - `pack_version` is the pack's own version, a dotted number raised whenever the pack changes (default `1`).
  - `app_versions` is the app versions it fits, as the app's MCP `serverInfo.version` says them: comma-separated clauses of `>=`, `>`, `<=`, `<` or `=` and a version (`>=0.20.2, <0.21`). `=0.20` takes any 0.20.x. A range that doesn't read fails the manifest.
- **Several packs for one app:** `packs/<id>.toml` is the newest, and `packs/<id>@<tag>.toml` is another pack for the same app, built in beside it (`manifest::VARIANTS`). When an engine is built, the app is started and its version read. `manifest::choose` then takes the newest built-in pack whose range the app meets, and rebuilds with it if it isn't the one asked for.
- **No pack fits:** the newest is used, with a warning in the build report and on stderr: "SoundCraft 4.0 isn't a version its pack (pack 3) was made for (…)". Nodes may send parameters the app doesn't take, or miss ones it needs. A pack loaded from a file is never swapped, only warned about.
- **What the engine says:** `EngineInfo.components` carries `pack` (its version) and `pack.app_versions`. The memo salt already holds the app's version and the pack's content.
- **Today:** all 13 packs are pack 1, each `>=` the app version they were last checked against (2026-11-06: PhotoCraft 0.20.2, VectorCraft 0.21.1, SoundCraft 0.17.1, …). `installed_apps_fit_their_packs` checks the installed apps.
- **When an app changes:** update the pack, raise its `pack_version`, and set `app_versions` to the new app version. To keep supporting the older app, first copy the old pack to `packs/<id>@<old app version>.toml`, with `app_versions` ending `<` the new version.

### 3.11 Crates and dependencies

- **MCP client: hand-rolled (D40),** about 400 lines on `std::thread` plus `serde_json`, reusing `plumcraft-engine`'s `rpc` framing (`crates/engine/src/rpc.rs`, the module spec 16.3 already names as shared with "the MCP adapter").
- Rejected: rmcp 3.5's client with `transport-child-process`. It needs tokio, which no PlumCraft crate uses, and every craft server it would talk to is plain newline JSON. It adds nothing a 400-line client lacks for stdio, and would matter only if a craft ever serves streamable HTTP.
- **Other dependencies:** `plumcraft-proto`, `plumcraft-engine`, `plumcraft-store`, `md5` (as the store uses), `toml` for the manifest, `libc` or `nix` for `killpg`, `png` (decoding thumbnails to `rgba8`, section 4). No `unsafe`.

### 3.12 Modes

| Mode | How | Use |
|---|---|---|
| 1, in-process | `plumcraft_craft::engine(manifest)` in the editor's host | tests; the children are still processes |
| 2, socket | `plumcraft-craft-engine --craft vectorcraft.toml`, launched by the editor's `ProcessHost` | **default**: an adapter fault is isolated from the editor |
| 3, HTTP | the SDK's `serve --listen` | a remote craft (a remote server), later: CAS uploads by `blob.has`, as D2 built; VectorCraft on Linux untested |

## 4. The editor side

- **Thumbnails:** on `preview`, the adapter exports a PNG of the document (or SVG) at `node_px` (`document.export {format: png, scale: node_px / max(width, height)}`) and decodes it into the `rgba8` rendition the canvas already draws (`crates/ui/src/canvas.rs:939-960`). It costs under 20 ms for the corgi trace (a 160 px PNG, measured with the CLI), and runs only for watched or thumbed nodes.
- **Image view (IMAGE outputs, docs and SVG as rasters):**
  - The Watched panel has no image viewer today (`crates/ui/src/watched.rs:106-130` handles audio, tables, ndarrays and text).
  - M3 adds the minimal one: an `rgba8` rendition at `inspect_px`, with zoom and pan.
  - It is the Image viewer of plan section 7 without probe or compare yet.
- **SVG view:**
  - Plan section 7's `plumcraft-view-svg` (usvg 0.48 + tiny-skia 0.12, external resources off). It reads the Display's `full` SVG DataRef and rasterises it at the current zoom, so zooming stays crisp. An outline toggle draws strokes only.
  - The engine's PNG rendition is the fallback and the reference. A difference between them is a renderer difference, not a bug in the graph.
  - Huge SVGs (78 MB) fall back to the rendition with a note.
- **Doc view:** the `doc` media kind (spec 10.1, "engine renditions + tree") shows the PNG rendition plus a `document.inspect {depth: 1}` summary (artboards, object counts, colours) as a JSON tree. The UI never parses the native file.
- **ROADMAP, "Specialist viewer windows per data type":**
  - These views are the first entries of that registry, keyed by `media`: `image`, `vector` and `doc`.
  - Clicking a preview opens the same viewer in its own window, as T16 did for the research software's picture.
  - M3 builds them as `MediaViewer`-shaped types inside `plumcraft-ui` (or `plumcraft-view-svg`), so the registry can adopt them without rework. The registry and the window are not in M.

## 5. Risks, open questions, decisions

### 5.1 Risks

| | Risk | Mitigation |
|---|---|---|
| R1 | Non-cancellable calls (trace, pathops) on big documents | Kill on cancel (D41); per-call timeout |
| R2 | Large documents: 218 MB native for one HFP trace; about 1 s per materialised node; durable-tier budget (D11) | Warm processes; lazy materialisation after measurement (D42); `memo.persist` off for docs by default |
| R3 | Purity is asserted by the manifest, not known | Run-twice conformance for every exposed node; `none` scope otherwise |
| R4 | Fonts and system state leak into text nodes despite roots | `machine` scope; a font-set digest in the salt if text nodes are added |
| R5 | Catalogue skew when VectorCraft is rebuilt (renamed keys, removed commands) | Binary digest in the salt; smoke test at engine start; `schema.changed` |
| R6 | The unexplained 120 s stall (section 1.7) | Timeouts and kill; report upstream if it recurs |
| R7 | Linked images in documents (paths outside the roots come in as `missingLinks`) | `vc.open` embeds on open (`file.place` with `link: false`) and surfaces the warnings as a `config` error |
| R8 | The default `mcp` mode attaches to a running desktop app | The adapter always passes roots, which imply headless |
| R9 | Version drift between note 05 and today | This note's tables come from live runs and are kept in `claude-debug/craft-mcp/` |

### 5.2 Open questions

1. Should the `doc` wire be one type per craft (`vectorcraft.doc`) or a generic `DOC` with `Form::Doc{app}` checked at run time? Per craft is proposed (D48).
2. Does warm reuse give the same bytes as cold reopen for every node? Open→save is the identity, but a command's effect on a live document versus a reopened one has not been tested for every command (M2 conformance case).
3. Multi-artboard documents: should exports take `artboard` (one), or produce a list (`range`) mapped over by the executor? Proposed: one by default, with a `list: true` output later.
4. The research software supporting www.virtimp.com picture → VectorCraft trace in one graph needs two engines, which D14 (one engine per workflow in v1) defers. M uses file and blob sources only.

### 5.3 Decisions for the maintainer

| | Question | Recommendation |
|---|---|---|
| D40 | MCP client | **Hand-rolled newline JSON-RPC over the child's stdio on std threads**, sharing `plumcraft-engine::rpc` framing; not rmcp (tokio, nothing gained on stdio) |
| D41 | Cancel | **Send `notifications/cancelled`, then SIGKILL after 200 ms and respawn** (under 50 ms); never wait on cooperative cancel, since trace ignores it and cancelled calls never reply |
| D42 | Materialising documents | **v1 saves and hashes every node's document; warm processes skip reopening; lazy (unsaved) intermediates only after an M2 measurement on the 218 MB case**, with a `Cx::demand()` SDK addition then. Touches D10/D11 (deferred): doc records are not persisted by default |
| D43 | Document identity | **`md5:raw:` of the uncompressed native save**, stored with its real extension in an adapter CAS that is the craft's read root; never craft-side gzip (unstable); PDF never content-keyed; `file.new` always with `created: null` |
| D44 | Selection | **A `select` STRING parameter on every art node** (`all`, `none`, `ids:`, `color:`, `kind:`, `json:`) plus an `ids` JSON socket, compiled to `select.*` before the command; no selection-producing nodes in v1 |
| D45 | Which commands are nodes | **24 curated nodes (section 2.3), 116 generated effect nodes from `effect.list`, and a `vc.run` escape hatch over a manifest allowlist**; not all ~190 inferred transforms |
| D46 | A craft crash, timeout or overlong reply | **Fail that node with kind `engine` and let independent branches continue**; amend spec 12.1, which calls `engine` run-level only |
| D47 | Memo salt and scope | **Salt `craft:<id>:<serverInfo.version>:<md5 of the binary>`** (no build hash is exposed); scope `machine` for manifest-pure nodes, `none` otherwise; never `global` (fonts, host numerics). Applies D9's typed canonical form to the params unchanged |
| D48 | Wire types | **Pack-namespaced `vectorcraft.doc`**; shared `IMAGE`, `SVG`, `PDF`, `JSON` so later multi-engine graphs (D14) can join on them |
| D49 | Modes and process | **Mode 2 by default** (`plumcraft-craft-engine --craft vectorcraft.toml`), mode 1 for tests, mode 3 later; one craft process per run in v1 (the executor is serial), pool of up to 4 for concurrent runs and sessions |
| D50 | Source proposals (golden rule 6; none made now) | **Record, don't send:** VectorCraft could expose `journal` in `CommandInfo`, put a build id in `serverInfo`, make trace and pathops cancellable with progress, answer a cancelled request with an error instead of silence, and default `file.new` `created` to null under automation roots; craft-libs `params_schema` and `effect` stay deferred (D15) |

**Earlier deferred decisions this work touches:**

- **D20** (craft MCP config paths, deferred "until the adapter work starts"): that point is now. The session that wrote this note again saw every craft MCP server fail with `ENOENT` at `/Volumes/src/artcraft/…`. Recommendation unchanged: fix the paths to `/Users/me/src/artcraft/…`, so the maintainer and agents can also drive VectorCraft directly while M2 is built.
- **D9, D10 and D11** (memo form, persistence, budgets): D47 applies D9 unchanged, and D42 and D43 keep docs out of persistence until D10 and D11 are decided.
- **D14** (one engine per workflow): it limits M to VectorCraft-only graphs.

## 6. Step plan for M2 and M3

### M2: `plumcraft-craft` (15 days, 13–17)

| Step | Content | Exit (conformance or test) | Days |
|---|---|---|---|
| M2.1 | MCP client: spawn, `initialize`, `tools/call`, progress, cancel, ping, bounded reads, errors | Unit tests against a fake craft (a Python stdio script in `claude-debug`); the hostile sweep gains MCP replies | 2 |
| M2.2 | Pool and supervision: roots, process groups, warm LRU, ping-before-reuse, timeouts, kill and respawn, stderr ring, binary-digest salt, five-crash stop | `kill -9` a child mid-node → that node `engine`, the next run succeeds; a timeout → `engine` and respawn | 2 |
| M2.3 | Craft CAS and DataRefs: staging of IMAGE/SVG sources, doc/SVG/PNG/PDF outputs, commonkit publication for the UI, path locators in the work root | Handles stable across two runs; open→save identity; a reaped commonkit SVG is a miss and republishes | 2 |
| M2.4 | Catalogue: the prose-grammar port (held to the 811 descriptions), type inference, typed catalogues, manifest TOML, the smoke-test gate | Every curated and generated NodeDef passes the smoke test on the corgi fixture; the schema digest is stable | 3 |
| M2.5 | Nodes: the selection compiler, doc nodes, query nodes, export nodes, `vc.run`, the error map, memo scopes, thumbnails | The raster → Trace (6 Colors) → Recolor → Export SVG graph runs and its SVG equals `vectorcraft-cli run`'s bytes | 3 |
| M2.6 | Conformance, mode 2 (and mode 1) | Spec 19 cases 3–7 and 12–14 on a VectorCraft graph: states and invariants; cancel mid-trace → `cancelled` within 500 ms and the child killed; a second run all `cached`; warm equals cold bytes; run-twice equality for every exposed node; a SoundCraft manifest lists its commands and runs `session.new` (genericity) | 3 |

### M3: in the editor (6–7 days)

| Step | Content | Exit | Days |
|---|---|---|---|
| M3.1 | Engine menu entry and manifest discovery for the craft engine; VectorCraft families in the palette | The palette lists the 24 nodes and the effects by menu | 0.5 |
| M3.2 | Image view in Watched (`rgba8` at `inspect_px`, zoom and pan) | A PNG output and a doc rendition show | 1.5 |
| M3.3 | SVG view (usvg + tiny-skia) with an outline toggle, falling back to the rendition | The traced corgi SVG renders crisp at 400 % | 2 |
| M3.4 | Doc view: rendition plus a summary tree; IMAGE source widget (drop or pick a file) | Dropping `test.jpg` onto a `vc.open` node works | 1 |
| M3.5 | P06 checklist (`docs/acceptance/P06-craft.md`) and the session's fixes | **The maintainer builds a trace-and-recolor graph by hand** | 1–2 |

**Not in M2 or M3:** lazy materialisation (unless the M2 measurement demands it), parallel branches in one run, the specialist-window registry, mode 3 against a remote server, SoundCraft nodes beyond the genericity smoke test, and any craft-libs or VectorCraft source change.
