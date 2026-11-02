# 19. How far is PlumCraft from ComfyUI?

The maintainer, 2026-10-31: "Start a separate research effort to examine how far plumcraft is from being able to implement the capabilities of 'comfyui'".

This note is research. Nothing here is built, and it changes no decision.

## Summary

**PlumCraft is days away from running real ComfyUI graphs, about two months away from being a good editor for them, and it should never try to replace ComfyUI's engine.**

The distance depends on what "implement the capabilities" means:

| Goal | Distance | Rests on |
|------------------------------------|------------------------|----------------------------------------|
| Run a ComfyUI graph from PlumCraft: its nodes in the palette, node status, cancel, the final image | **2 to 4 agent-days**, after a one-day SDK fix. No core change | One adapter engine of 800 to 1,500 lines of Python over ComfyUI's HTTP and websocket API; generator, manifests and hosts exist |
| A daily driver for ComfyUI's core nodes: previews while sampling, seed control, workflow and PNG import, queue, history | **6 to 10 agent-weeks** in total | The stages in section 6; most of the work is generic editor features that any slow engine wants |
| Most custom nodes | Their Python comes free with the adapter. **Their JavaScript never will** | A custom node's frontend is JS loaded by a browser (`WEB_DIRECTORY`); PlumCraft is a native egui app |
| The same capabilities with no ComfyUI behind them | **Open-ended, and a bad idea** | About 215,000 lines of Python, 108 model configurations, 55 model families, releases about every two weeks |

**Cheap:** catalogue mapping from `/object_info`, node status, cancel, importing API-format workflows (a PNG carries one), wire bends for reroutes, net labels for Set and Get, groups for subgraphs, Vary-style sweeps.

**Expensive:** queue and history as a real editor model, ComfyUI workflow import with all its positional and virtual-node quirks, video and 3D viewers, mask painting.

**Out of reach:** ComfyUI's JavaScript extensions and custom widgets in a native editor.

**Bad ideas:** putting MODEL or CLIP objects through the content-addressed store; reimplementing ComfyUI's memory manager or its model zoo; native Rust inference as the route to parity; loading ComfyUI into the editor's process, which would put the editor under the GPL.

**Recommendation:** strategy (a1), an adapter over ComfyUI's HTTP and websocket API, in its own repository, with ComfyUI running unmodified beside it. PlumCraft stays MIT. Before anything else, fix one real bug this research found: watching a value that isn't JSON or bytes kills a Python engine (section 2.2).

## How this was checked

| Mark | Meaning |
|---|---|
| **[S]** | Verified by reading source: PlumCraft's working tree, or ComfyUI's |
| **[P]** | Verified by running a probe against PlumCraft's Python SDK |
| **[D]** | Taken from ComfyUI's or a third party's documentation |
| **[I]** | Inferred from the above; not observed |
| **[?]** | Could not be determined |

- **ComfyUI:** shallow clone of `github.com/comfyanonymous/ComfyUI`, commit `0dbfd49` (2026-10-31), version 0.43.0, pinned frontend `comfyui-frontend-package==1.57.6`. ComfyUI was **not run**: no models, no pip install.
- **The frontend:** `Comfy-Org/ComfyUI_frontend` is a separate repository and was not read. Statements about it come from docs, from workflow files in the core repository, and from the backend's expectations.
- **PlumCraft:** the working tree on 2026-10-31, including the uncommitted net labels. Claims marked [S] were checked in code; where docs and code disagreed, the code is reported.
- **Probes:** a small Python engine with a live, unserialisable object on a pack wire type, run in process and over the real HTTP binding, with a scratch store. Scripts are in the session's scratchpad, not the repository.
- **No cargo build or test was run.**
- **ComfyUI's model names** below are quoted from its README. They were not checked against anything else.

**Effort classes.** Sizes are lines of code including tests; time is agent-assisted time with one maintainer reviewing.

| Class | Size | Time |
|---|---|---|
| S | under 500 lines | a day or less |
| M | 500 to 2,000 lines | 1 to 3 days |
| L | 2,000 to 8,000 lines | 1 to 3 weeks |
| XL | more, or never finished | months, or ongoing |

**What the estimates rest on:** note 15's per-step sizes (300 to 900 lines each, all built), the research software's plan's section sizes, and this repository: about 65,000 lines of Rust and 7,000 of Python in 50 commits over five calendar days (2026-10-27 to 31). Do not extrapolate that pace. Work that needs real models is slower, because each check means downloads and minutes of sampling, and CI has no GPU. Treat every range as plus or minus a factor of two.

## 1. ComfyUI's capabilities against PlumCraft's

The **Effort** column is the cost under the recommended strategy (a1). "Comes with" means ComfyUI does it behind the adapter and PlumCraft only needs to show it. **Where** is core, SDK (either PlumCraft SDK), interface (`docs/interface.md`), or pack (the adapter, outside this repository).

### 1.1 Execution

| Capability | ComfyUI (evidence) | PlumCraft | Effort | Where |
|---------------|---------------------------------|------------------------------|---------------|-------|
| Partial re-execution | Output cache keyed by class, `fingerprint_inputs`, inputs and ancestors: `caching.py:110` [S] | **Have:** plan keys, `cached`, spec 14, `exec.py` [S] | Comes with | Pack |
| Cache policies | Classic, LRU, RAM pressure (the default), none; pluggable providers: `execution.py:114`, `main.py:349` [S] | **Partial:** 1,024 entries, no size limit, `exec.py` `Memo` [S]; persisted records survive restarts, which ComfyUI's default doesn't | Comes with; M for strategy (b) | SDK |
| Lazy inputs | `check_lazy_status`, make_input_strong_link: `execution.py` [S] | **Missing:** every input resolves first; `_needed` works on wires only [S] | Comes with; M for (b) | SDK and spec 7.6 |
| Node expansion | A node returns a new subgraph at run time; `GraphBuilder`, ephemeral nodes, `display_node`: `execution.py`, `graph_utils.py` [S] | **Missing:** SchemaReply.expansions exists but nothing reads it; the research software's plan sizes `node.expand` (656 lines) [S] | Comes with; the pack must fold ephemeral ids onto display nodes (S) | Pack |
| Loops | Built on expansion: `nodes_loop.py`, `FLOW_CONTROL` [S] | **Missing:** a cycle is a graph error, spec 7.6 rule 1 [S]. A loop's back wire would need editor support | Comes with for imported graphs; M to draw and edit | Core |
| Conditions | `Switch`, `SoftSwitch`, `Branch`, `ExecutionBlocker`: `nodes_logic.py`, `graph_utils.py:140` [S] | **Spec only:** rule 6 (absent values); neither executor has it [S] | Comes with; M for (b) | SDK |
| Batches and lists | `INPUT_IS_LIST`, `OUTPUT_IS_LIST`; the shorter list repeats its last item, `execution.py:262` [S]. Batches also live inside tensors | **Have, with other rules:** zip on a shared axis, product across axes, mismatch an error, spec 7.6 rule 2 [S] | Comes with; sweeps from Vary into ComfyUI need M | Pack |
| Subgraphs | definitions.subgraphs in workflow files [S]; a frontend feature, flattened before execution [I] | **Have:** groups with promoted widgets, enter and leave, nesting (E3) [S] | S to map on import | Pack |
| Async nodes | `pending_async_nodes`, external blocks: `execution.py` [S] | **Missing** in the SDKs; the run is one blocking thread [S] | Comes with | None needed |
| Validation before a run | `validate_prompt`, `validate_inputs`: `execution.py` [S] | **Partial:** wires typed while editing; the rest is a run-time `graph` or `config` error [S] | S: map `node_errors` | Pack |
| Cancel | `/interrupt`, an exception checked in sampler callbacks [S] | **Have:** `run.cancel`, spec 11, conformance [S] | S | Pack |
| Per-node progress | `progress`, `progress_state`: `main.py:475`, `progress.py:184` [S] | **Have:** `progress` with `fraction` and `stage` [S] | S | Pack |

### 1.2 Data types

ComfyUI passes **live Python objects** between nodes in one process. Node outputs are held in the cache as they are: `cached.outputs[output_index]` is handed straight to the next node (`execution.py`) [S]. PlumCraft passes content-addressed bytes between engines, and only between engines.

| ComfyUI type | What it is | A byte form? | In PlumCraft |
|----------------------------|----------------------------------|--------------|----------------------------|
| `MODEL`, `CLIP`, `VAE`, `CONTROL_NET`, `CLIP_VISION`, `STYLE_MODEL`, `GLIGEN` | Live objects; `MODEL` is a `ModelPatcher` sharing weights with its clones (model_patcher.py:435) [S] | **No, and it shouldn't have one** | Opaque pack types that never leave the engine |
| `CONDITIONING` | Lists of tensors plus dictionaries of options [S] | Not a stable one | Opaque |
| `LATENT` | A dictionary holding a `samples` tensor and options [S] | Possible, no reason to | Opaque |
| `SAMPLER`, `SIGMAS`, `NOISE`, `GUIDER`, `HOOKS` | Objects with methods; `SIGMAS` is a tensor [S] | Mostly not | Opaque |
| `IMAGE` | Float32 tensor, batch first, height, width, channels [S] | **Yes:** an `md5:nd1:` array | Pack type with a conversion pair to core `ARRAY` |
| `MASK` | Float32 tensor, batch, height, width [S] | Yes | Same |
| `AUDIO` | Dictionary of `waveform` and `sample_rate` [S] | Yes | Pack type with a conversion pair to core `AUDIO` |
| `VIDEO` | An object wrapping a file or frames [S] | The file | Blob of an encoded video; PlumCraft has no viewer |
| `MESH`, `VOXEL`, `FILE_3D_*`, `SPLAT` | Tensors or file bytes [S] | The file | Blob; no viewer |
| `INT`, `FLOAT`, `STRING`, `BOOLEAN`, `COMBO` | Scalars and choices | JSON | `INT`, `FLOAT`, `STRING`, `BOOL`, `ENUM` |
| `*`, `MATCHTYPE`, comma-joined types, `AUTOGROW`, `DYNAMICCOMBO` | Dynamic typing [S] | Not applicable | See section 2.4 |

**Name collisions (a trap).** ComfyUI declares `IMAGE`, `AUDIO`, `SVG` and `ARRAY` with different meanings from PlumCraft's core types of those names [S]. PlumCraft's `IMAGE` is an encoded file and its `AUDIO` is (frames, channels) float32. Rule 3 and `every_engine_declares_the_core_types_exactly` forbid re-declaring them, so the adapter must rename every ComfyUI type, for example `comfy.IMAGE`, as `vectorcraft.doc` does.

### 1.3 Model management and samplers

| Capability | ComfyUI (evidence) | PlumCraft | Effort | Where |
|---------------|---------------------------------|------------------------------|---------------|-------|
| Model discovery | `folder_paths.py`, extra_model_paths.yaml, `/models` [S] | **Missing.** F85 keeps it a pack matter | Comes with | Pack |
| Model loading | 108 model configurations in `supported_models.py`; 55 families under `comfy/ldm` [S] | **Missing** | Comes with | Pack |
| VRAM states, offload, partial load | `VRAMState` with six states, `load_models_gpu`, `free_memory`, `partially_load`: `model_management.py`, `model_patcher.py` (4,500 lines) [S] | **Missing** | Comes with. XL to reproduce | Don't |
| Weight streaming | `comfy-aimdo`, Nvidia and AMD on Windows and Linux [D] | **Missing** | Comes with where supported | Don't |
| LoRA patches | `add_patches` on a clone that shares weights [S] | **Missing** | Comes with. Hard in (b): diffusers mutates a pipeline [D] | Pack |
| Samplers | 45 in `SAMPLER_NAMES` [S] | **Missing** | Comes with | Pack |
| Schedulers | 9 in `SCHEDULER_HANDLERS` [S] | **Missing** | Comes with | Pack |
| Apple Silicon | MPS device support, `SHARED` memory state; listed in the README [S] | Not applicable | Comes with | Pack |

### 1.4 Nodes and ecosystem

| Capability | ComfyUI (evidence) | PlumCraft | Effort | Where |
|---------------|---------------------------------|------------------------------|---------------|-------|
| Built-in nodes | About 1,000: 127 V1 and 536 V3 local, 334 partner nodes, by regex, so tests included [S] | Hundreds across engines, none generative [S] | Comes with | Pack |
| Node API | V1 (`INPUT_TYPES`, `RETURN_TYPES`) and V3 (`io.Schema`, class methods); new schema features V3 only [S, D] | `@node` typed methods (`declare.py`), low-level `Node` [S] | S: `/object_info` flattens both | Pack |
| Custom nodes, Python | Anything in `custom_nodes/`, arbitrary code, in-process [S] | Separate engines, each its own process and project [S] | Comes with | Pack |
| Custom nodes, JS | `WEB_DIRECTORY` served to the browser: `nodes.py:2299` [S] | **Out of reach** | Never. Offer "Open in ComfyUI" (S) | Pack |
| Install and update | ComfyUI-Manager, comfyui_manager==4.2.2, `--enable-manager`; Comfy Registry [S, D] | Manifests in ~/.config/plumcraft/engines, Reload Engine List; no installer UI [S] | Delegate to Manager | Pack |
| Blueprints and templates | 117 subgraph blueprints in `blueprints/`; comfyui-workflow-templates [S] | `templates` in the spec; no UI reads it [S] | M | Core |
| Engine tests | None found | **Have:** `plumcraft conform`, `plumcraft-engine check` [S] | A whole-graph adapter needs a profile (S) | Core |

### 1.5 Server, auth and cloud

| Capability | ComfyUI (evidence) | PlumCraft | Effort | Where |
|---------------|---------------------------------|------------------------------|---------------|-------|
| Queue a graph | `POST /prompt`, `PromptQueue` with priority, `front`: `server.py:1164`, `execution.py:1306` [S] | `run`; `queued_behind`; the editor supersedes instead [S] | S for the adapter; L for the editor (section 4) | Pack, core |
| Progress stream | `/ws`: `executing`, `executed`, `progress`, `progress_state`, `execution_*`, `status` [S, D] | Events, spec 9; push, long-poll, poll [S] | S | Pack |
| Binary previews | Frame types 1, 3, 4 in `protocol.py`; JPEG at 512 px by default [S] | Spec 10.6 designs it; **not implemented** (section 2.5) | M | Core, SDK |
| Catalogue | `GET /object_info`: `server.py:892` [S] | `schema` [S] | S | Pack |
| Files | `/view`, `/upload/image`, `/upload/mask`: `server.py` [S] | DataRefs, spec 13 [S] | S | Pack |
| History, jobs | `/history`, `/api/jobs`, `jobs.py` [S] | **Missing:** snapshots of the last 16 runs only [S] | L | Core, pack |
| Free memory | `POST /free` [S] | None | S: a pack command | Pack |
| Local auth | **None by design:** "Anyone with access to the ComfyUI URL is trusted"; `--multi-user` is per-user storage, not access control: `SECURITY.md`, `cli_args.py` [S] | **Better:** 256-bit token on sockets, bearer token off loopback, same-host proof [S] | None | Core |
| Partner nodes | 334 classes; auth_token_comfy_org, `api_key_comfy_org` in `extra_data` [S] | None | S: pass a key from the manifest's env | Pack |
| Cloud | `cloud.comfy.org`, `X-API-Key`, same message types; v1 deprecated for API v2 [D] | HTTP engines can be remote [S] | M: a second target for the adapter | Pack |

### 1.6 Media

| Capability | ComfyUI (evidence) | PlumCraft | Effort | Where |
|---------------|---------------------------------|------------------------------|---------------|-------|
| Images, 16-bit, EXR | README, `nodes_images.py` [S] | Picture viewer: PNG, JPEG, TIFF, WebP, GIF [S]; EXR unknown [?] | S to M for EXR | Core |
| Audio generation and playback | `nodes_audio.py`, ACE-Step and others [S] | **Have:** waveform, spectrogram, playback (F3) [S] | S: conversion pair | Pack |
| Video | `nodes_video.py`, Wan, LTX-Video, HunyuanVideo; `av` [S] | **Missing:** `value.open` hands it to another app [S] | L: a frame viewer with a decoder | Core |
| Large values | Live tensors, no copies | **Limit:** `MAX_READ` is 128 MiB in both SDKs; 81 frames of 720p float32 is 2.6 GiB [S] | Keep files, not frames, at the border | Pack |
| 3D | `Load3D` in the frontend, `nodes_save_3d.py`, Hunyuan3D, splats [S] | **Missing:** spec has `mesh`, no viewer [S] | L: wgpu viewer, as SolidCraft has | Core |
| Text generation | Gemma, Qwen and others in README [S] | Text values, `Append` designed, not built [S] | S to M | SDK |

## 2. The central question: how would a "ComfyUI engine" work?

### 2.1 The mismatch is smaller than it looks

The brief states the mismatch as bytes against live objects. That is true **between** engines. It isn't true **inside** a Python engine. PlumCraft's Python executor keeps a `values` dictionary of whatever each node returned and passes those objects straight to the next node. Only targets, watched ports and thumbnails become bytes. The memo holds the objects too, and it keys on plan keys, not on values.

So a pack wire type already carries a live object:

| Probe | Result |
|------------------------------------------|----------------------------------------------------------|
| Load (returns an unserialisable object) wired into Use, with the output of Use as sink | Works; object arrives at Use [P] |
| Change Use's seed, rerun | Load is `cached`, the same object is reused, nothing reloads; ComfyUI's behaviour [P] |
| A load slower than the 1 s persist threshold | No crash; rerun in the same process is `cached` [P] |
| Rust `portable()` for a same-engine wire | Always allowed [S] |

The real constraint is the one the research software's plan already found. A value that isn't bytes can't cross a fragment boundary, **even back to the same engine**: `runner.rs` skips the node when the crossing value is opaque. A ComfyUI graph that detours through the array engine and returns would lose its MODEL unless the research software's plan's `values.held` capability is built.

### 2.2 A bug found on the way: engine-local values crash the engine

| Probe | Result |
|------------------------------------------|----------------------------------------------------------|
| Watch a port holding a live object, in process | `run` fails: "the engine crashed (TypeError: Object of type Live is not JSON serializable); restarting" [P] |
| The same output as a `target`, over HTTP | `run.get` drops the connection; the server thread dies encoding the reply [P] |
| The same output as a `target`, in process | The reply holds `{"kind": "json", "value": <Live>}`, a live object in a message [P] |

Cause: `data.Plane.port_value` falls through to `kind: json` for anything it doesn't recognise, and `exec._summarize` calls `json.dumps` on it [S]. The Rust `Data` enum can't hold such a value, so this is Python only [I].

This breaks golden rule 1. It matters more than it looks, because **hovering a wire peeks at it** (`wire_view.rs`, `view.peek`), so hovering a MODEL wire would kill the engine and drop its memo, and with it every loaded model [I]. Spec 4.5 already has the right answer: `{"kind": "opaque", "type": "...", "summary": "..."}`. The fix is S, needs no interface change, and is worth making whatever happens with ComfyUI.

### 2.3 Strategies

**(a1) An adapter over ComfyUI's HTTP and websocket API.** ComfyUI runs unmodified as its own server. A generated Python engine maps `/object_info` to the catalogue, translates PlumCraft's graph to API-format JSON, posts it to `/prompt`, and maps websocket events to status records.

- **Whole-graph execution.** ComfyUI must run the graph: lazy inputs, expansion, its cache and its model manager live in its executor. The SDK's `exec.run` can't be bypassed today (`host.py:373`), so the SDK needs a small seam, `Engine.run_graph(req, emit, cancel)` (S). The Craft adapter drives per node and doesn't need it.
- **Watching any port.** ComfyUI only reports values from output nodes. To show an `IMAGE` on any wire, the adapter adds a hidden `PreviewImage` to the prompt for that run. Upstream is cached, so this is cheap [I]. MODEL and other opaque ports show a summary only.
- **Crossing engines.** Out: fetch by `/view`, publish as a blob. In: `/upload/image` plus a hidden `LoadImage` node. Files are cheap; float tensors lose precision through 8-bit PNG, the quantisation hazard the research software's plan names.
- **Risks:** one more process to supervise, and an API that moves. ComfyUI's server has no auth, so any local process can drive it. Node status is coarser than PlumCraft's: there is no `done` for non-output nodes, so the adapter infers it from the next `executing`.

**(a2) Importing ComfyUI as a library in a Python engine.** `PromptExecutor` takes a `server` implementing the small `ExecutionServer` protocol (`execution.py:667`, `server_protocol.py`), so the executor can be driven without the HTTP server [S]. But built-in nodes import `server.PromptServer` directly and call `PromptServer.instance.send_progress_text` (`nodes_audio.py`, `nodes_loop.py`, others) [S], and custom nodes register routes on it (`example_node.py.example`) [S]. The real class must therefore exist.

- **Gain:** direct access to the output cache, so any tensor on any port can be read without hidden nodes, and no second server.
- **Cost:** the engine tracks ComfyUI's internals, not its API; `main.py` startup has to be reproduced; a segfault in a custom node takes the engine down (PlumCraft recovers, but every model reloads); and the engine process is GPL (section 2.8).
- **Verdict:** a later option, only if hidden preview nodes prove too weak.

**(a3) PlumCraft's executor calling ComfyUI's nodes one at a time.** Rejected. It loses lazy inputs, expansion, async nodes, hidden inputs and the model tracker, and it reimplements `execution.py` for no gain.

**(b) A native Python engine on diffusers.** Apache-2.0 [S], no GPL question, an ordinary generated engine with `Pipeline`, `Prompt` and `Sample` nodes. Live objects work (section 2.1) once the bug is fixed.

- **Gain:** a graph-native design, and PlumCraft's memo, thumbnails and watch list work as for any other engine.
- **Limit:** pipelines are coarse. ComfyUI's graph vocabulary (MODEL patched by LoRA, CONDITIONING combined, a sampler chosen separately) doesn't map to pipeline calls, so ComfyUI workflows won't import and custom nodes won't run. Memory management is diffusers' `enable_model_cpu_offload` [D], and the SDK's memo pins live models forever unless it gains size-aware eviction (M).
- **Status on MPS:** diffusers documents `mps`; an old issue reports Flux failing there; current status [?].
- **Verdict:** a fine small demo of "generative AI in PlumCraft, MIT all the way". It isn't a route to ComfyUI's capabilities.

**(c) Native Rust inference.** candle has SD 1.5, 2.1, XL, Turbo and Flux examples [D], and is Apache-2.0 per GitHub [S]; burn and ort likewise [S]; `stable-diffusion.cpp` is MIT [S].

- **Gain:** one binary, no Python, rule 1 by construction, a good in-process demo.
- **Limit:** every architecture, sampler and patch mechanism has to be ported, against 215,000 lines moving on a two-week release cycle [S], and custom nodes are gone. candle's example README shows CUDA and CPU and doesn't mention Metal [D], and Metal is what the maintainer's Mac needs [?].
- **Verdict:** bad as a parity route. A single-model engine (SDXL Turbo) is a reasonable side project, and nothing more.

| | (a1) HTTP adapter | (a2) Library | (b) diffusers | (c) Rust |
|---|---|---|---|---|
| ComfyUI core nodes | All | All | None; a chosen subset of models | None; a chosen few |
| Custom nodes (Python) | All | All | None | None |
| Custom nodes (JS) | No | No | No | No |
| Workflow import | Yes | Yes | No | No |
| Memory management | ComfyUI's | ComfyUI's | Diffusers', plus SDK work | To write |
| Intermediate tensors | Through hidden nodes | Direct | Direct | Direct |
| Previews while sampling | Websocket frames | Callback | Callback | Callback |
| PlumCraft's licence | MIT, unaffected | MIT; engine GPL | MIT | MIT |
| Crash isolation | Two processes | One engine process | One engine process | In-process or one process |
| Tracks ComfyUI's | API | Internals | Nothing | Nothing |
| First result | M | L | M to L | L |
| To breadth | L | L | XL | XL, never complete |

### 2.4 Catalogue mapping from `/object_info`

`node_info` returns `input` (required, optional, hidden), `input_order`, `output`, `output_is_list`, `output_name`, `display_name`, `description`, `category`, `output_node`, `search_aliases`, and more (`server.py:845`) [S]. V3 nodes appear in the same shape through `GET_NODE_INFO_V1` [S].

| ComfyUI | PlumCraft | Note |
|------------------------------|------------------------------|----------------------------------------|
| Class name `KSampler` | `comfy.KSampler` | Conformance requires the engine's prefix [S] |
| `display_name`, `description`, `category` | `display`, `description`, `category[]` | Split on `/` |
| Wire type `MODEL` | Wire type `comfy.MODEL`, media `bytes` | Opaque; colour by hash |
| `INT`, `FLOAT` with `min`, `max`, `step`, `round` | `min`, `max`, `step`, `decimals` | Direct |
| `STRING` with `multiline` | `STRING` | **No multi-line text widget** except path lists [S]; every prompt needs one |
| `COMBO` or a list | `ENUM`, `choices` | Direct; model lists may be long |
| `BOOLEAN` | `BOOL` | Direct |
| `forceInput`, `socketless` | `socket` | Direct |
| `tooltip`, `advanced` | `tooltip`, `advanced` | Direct |
| Hidden inputs | Dropped | Filled by ComfyUI from `extra_data` |
| control_after_generate | New catalogue field | Section 4 |
| `*`, `MATCHTYPE`, `IMAGE,MASK` | `ANY` by the `compat` convention, spec 7.2 | Weaker wire checks than ComfyUI's |
| `AUTOGROW` | `variadic` | Direct in Rust; "no variadics" in the Python executor doesn't matter with a whole-graph seam |
| `DYNAMICCOMBO` | `options`, `active_when` | `active_when` is in `Port`, but `crates/ui` and `crates/graph` never read it [S] |
| `output_is_list` | `list: true` | Display only under a whole-graph seam |
| `output_node` | `flags` | Used as targets |
| Unknown widget types | Text field, with a warning | Custom JS widgets end here |

Two practical limits. `schema` must fit `limits.max_frame`, 16 MiB [S]; `/object_info` with many custom nodes and long model lists may approach it [?]. And when a model file appears, ComfyUI's lists change, but PlumCraft has no handler for `schema.changed` (no reference in `crates/session`, `crates/ui` or `crates/host`) [S], so the adapter needs Reconnect until that exists (S).

### 2.5 Previews while sampling

**The interface carries them; nothing implements them.** Spec 10.6 defines `node.live` with `snapshot`, `append` and `log`. `crates/proto/src/event.rs` has the types. No SDK can emit one (`Cx` offers only `cancelled`, `setup_took`, `progress`) and neither `crates/session` nor `crates/ui` handles `Event::Live`; only the CLI prints `Log` updates [S]. `live.snapshot` is never advertised [S].

ComfyUI's side is easy. `extra_data.preview_method` turns previews on per prompt (the server default is `NoPreviews`, `cli_args.py:136`, `execution.py`) [S]. Frames are JPEG, longest side 512 by default, quality 95 (`server.py:1441`) [S]. A client that declares `supports_preview_metadata` gets frame type 4 with JSON naming the node; otherwise type 1 carries no node id [S].

| Option | Change | Notes |
|------------------------------------|--------------------|--------------------------------------------|
| Repeated `preview` status records carrying a `thumb` | **None in core** | The reducer stores a thumb whatever the state and the canvas draws `n.thumb` [S]. 160 px only. A `preview` may advance the node's state, freezing the bar; not run [I]. A good first step |
| `node.live` snapshot whose Display has `media: image` and a JPEG blob in `full` | SDK emit, session and panel handling (M) | No interface change: the picture viewer reads JPEG [S]. Publishing a JPEG per step is cheap |
| The spec's Rgba8 rendition per step | Same | About 1 MiB per step at 512 px; fine locally, heavy remotely |
| A new encoded rendition kind | Interface, additive capability (S) | Only for remote. Spec 13.3 reserves `preview_codec` for thumbs; `Rendition` has no encoded form [S] |

Spec 10.6 limits live updates to watched values. ComfyUI shows them on the sampling node unasked. The editor can watch the running node automatically while a run is in flight; that is an editor choice, not an interface change.

### 2.6 Memo and cache

Under (a1), **ComfyUI's cache is the memo.** The adapter reports `cached` from `execution_cached` and doesn't use the SDK's `Memo`, because caching a MODEL in two places is pointless and caching outputs without ComfyUI's knowledge breaks its model tracker [I].

- **Identity across engines still works.** Published outputs have content handles, so a downstream engine's bound-input memo hits when a fixed seed reproduces the same bytes (spec 14.2).
- **Salt.** ComfyUI's signature holds no code digest (`caching.py:110`) [S]. An adapter that claims nothing about persistence has no salt problem. PlumCraft's stricter rule is one reason its persisted records are safer.
- **`memo.scope`.** Comfy's `fingerprint_inputs` plays the part of `Node.volatile`. A node with a randomised seed needs a fresh seed from the editor, not `scope: none`.

Under (b), the SDK's `Memo` keeps up to 1,024 live results, so a loaded model stays pinned for the engine's life. It needs eviction by size, as ComfyUI's RAM-pressure cache does, and probably a release hook (M).

### 2.7 Workflow import

ComfyUI has two formats and a PNG carries both.

- **API format** (`prompt`): `{id: {class_type, inputs}}`, links as `[node, slot]`, no positions. PNG text chunk `prompt`: `nodes.py:1713` [S]. A translator is **S**: positions come from Tidy.
- **Workflow format** (`workflow`, in `extra_pnginfo`): `nodes`, `links`, `groups`, `definitions.subgraphs`, `reroutes`, `extra`, versions 0.4 and 1.0: `default_workflow.json`, `blueprints/*.json` [S], spec page [D]. A translator is **L**, because:

| Trap | Detail |
|------------------|----------------------------------------------------------------------------------|
| Widgets are positional | `widgets_values` is a list; KSampler holds `[42, "fixed", 8, 1, "res_multistep", "simple", 1]`, seed then a phantom control_after_generate value [S] |
| It needs `object_info` | Position maps to name through `input_order`, so import needs ComfyUI running or a cached catalogue [I] |
| Virtual nodes | Reroute, PrimitiveNode, Note, and custom Set and Get exist only in the frontend [I] |
| Modes | `mode` holds mute and bypass [D]; the frontend applies them when it builds the prompt [I] |
| Subgraphs | Definitions with their own inputs, outputs and nested links; flattening logic is frontend code [I] |
| Layout | Keep `pos`, map subgraphs to groups, group boxes to frames (PlumCraft has none), native reroutes to `via` bends |
| App Mode | Lives in `extra`; keep it in `meta` so a round trip can restore it [D] |

The translator belongs in the pack, not core (rule 3). Importing the file needs a small generic path in core: an engine declares that it can open a media type, and the editor offers it. `x.<pack>.<op>` exists in the spec, but the Python host refuses any unknown op and neither host dispatches `x.` ops [S], so **pack operations themselves need building** (S to M).

Reimplement `graphToPrompt` from the format, not from the frontend's source: that code is GPL-3.0 (section 2.8).

### 2.8 Licensing

**Facts.** PlumCraft is MIT (`LICENSE`, workspace `license = "MIT"`) [S]; plan G11 mentions MIT/Apache dual for the future [S]. ComfyUI, its frontend and ComfyUI-Manager are GPL-3.0 [S]. `comfy-aimdo` shows `NOASSERTION` on GitHub and no licence on PyPI [?]. diffusers, candle, burn and ort report Apache-2.0, `stable-diffusion.cpp` and k-diffusion MIT [S]. Models carry their own licences, some with territorial limits [D].

**The rule that decides.** The GPL's duties attach when software is conveyed. Running ComfyUI privately, in any arrangement, needs nothing. The questions below matter if PlumCraft or an engine is distributed. PlumCraft's repository is private today. The FSF's FAQ says pipes, sockets and command-line arguments are "normally used between two separate programs", unless "the semantics of the communication are intimate enough, exchanging complex internal data structures"; modules "designed to run linked together in a shared address space" are almost surely one program; and it treats interpreted programs and their libraries as linked [D]. This is reading, not legal advice.

| Strategy | PlumCraft editor and SDK | The adapter or engine | Conditions |
|----------|--------------------|--------------------------|--------------------------------------------|
| (a1) HTTP | Stay MIT | May be MIT if it holds no ComfyUI code | JSON graphs and image files over HTTP are ordinary program-to-program communication. Don't copy frontend or backend code into it. If ComfyUI is bundled, that's an aggregate: ship its source and licence and keep it separate |
| (a2) Library | Stay MIT: the socket separates them | **GPL-3.0 when distributed.** `plumcraft_engine` being MIT is compatible, in that direction | Keep it in its own repository. Never load it in-process in the editor (mode 1, or via pyo3): that would make the editor GPL |
| (a3) | As (a2) | As (a2) | Rejected anyway |
| (b) diffusers | MIT | Any compatible licence | Apache-2.0 is compatible with MIT work. Don't borrow ComfyUI's samplers; k-diffusion is MIT |
| (c) Rust | MIT | MIT or Apache-2.0 | Candle and friends are compatible; a port of ComfyUI code isn't. Model licences still apply |

**In every strategy:** generated catalogues are facts about a running server, not copied code. Translating a ComfyUI workflow means reading a data format. Porting a function from the frontend is the thing to avoid.

## 3. Interface and SDK work

| Change | Kind | Needed by | Effort |
|------------------------------------------|----------------------------|--------------------|----------|
| Live objects become `opaque`, never `json`, in `port_value`, `_summarize` and reply building; a test | SDK bug fix, no spec change | Every strategy | S |
| `Engine.run_graph`: an engine may run a whole graph itself | SDK seam, both executors optional; no message change | (a) | S |
| `Cx.live` and session handling of `node.live` snapshots | Implements spec 10.6 | Previews; also useful to the research software's stages | M |
| `x.<pack>.<op>` dispatch and a command to reach it | Implements spec 15 | Import, pack commands such as Free Memory | S to M |
| `schema.changed` handling in the session | Implements spec 7.4 | Model lists | S |
| `values.held` capability | Note 18's proposal, additive | Graphs that leave and re-enter one engine | M |
| A catalogue field for after-run value control (seed) | Additive to Port | Seeds | S |
| A multi-line flag on `STRING` ports | Additive to Port | Prompts | S |
| Encoded rendition kind | Additive capability | Remote previews only | S |
| Queue and history operations | New capability, for example `run.queue`, `run.history` | Queue and history panels | M |
| A conformance profile for whole-graph engines | `crates/conform` | The adapter holding `plumcraft conform` | S |

Each message change goes into `docs/interface.md` in the same commit (rule 5). None of this names ComfyUI, so rule 3 holds: each item is generic and useful to other slow or GPU engines. Every new parser joins the hostile-input sweep, and the adapter's parsing of ComfyUI's messages should treat them as hostile (rule 1).

**One note on auto-run and supersede.** The editor's latest-wins model cancels a run when a new one starts (spec 8.2). Right for milliseconds; hostile when a sample takes two minutes and an edit throws it away. Auto-run should default off for slow engines, or a run should be able to finish.

## 4. Editor gaps, whatever the engine

| Item | ComfyUI | PlumCraft today | Effort | Notes |
|---------------|----------------------------|------------------------------|-------|--------------------|
| Multi-line text widget | Yes | **Missing:** only path lists wrap (`widgets.rs:201`) [S] | S | Every prompt needs it |
| Seed control | `fixed`, `randomize`, `increment`, `decrement`, applied after a run | **Missing** [S] | S to M | The chosen seed must be written back, or results can't be reproduced |
| Long combos | Model library sidebar [D]; combo search [?] | **Partial:** plain `ComboBox` [S] | S | A searchable popup, as the palette |
| Widget zoo | Colour, curve, bounding box, image compare, webcam, 3D, histogram [S] | Numbers, text, enum, bool, path [S] | M each | Do by demand. The research software's curve editor is one |
| Input image preview | Upload and preview on the node | **Partial:** path field and dialog; thumb after a run [S] | S to M | |
| Reroutes | Native reroutes in the file [D] | **Have:** wire bends with `via` [S] | S to map | |
| Set and Get | Custom node pack | **Have, better:** net labels, jump to driver, rename [S] | S to map | |
| Subgraph UX | Enter, promote, publish, nest, navigation bar [D] | **Have mostly:** groups, promoted widgets, nesting [S] | M for a library | Blueprints are `graph.insert` plus a palette section |
| Frames, coloured boxes | `groups` in the file | **Missing:** no frames; the research software's plan lists them [S] | M | Needed for faithful import |
| Notes | Note, Markdown Note | **Missing** [S] | S | |
| Bypass and mute | Ctrl+B, Ctrl+M [S] | **Missing:** no such field on `Node` [S] | M | Generic: the session rewrites the graph before sending |
| Node colour, pin | Yes [S] | **Missing** | S | |
| Queue panel | Run bar, batch count, queue modes, front of queue, active jobs [D] | **Missing:** one run, supersede [S] | L | The biggest model change here |
| History and assets | Job history, media asset library with reuse [D] | **Missing** | L | A gallery of past outputs by handle fits the store well |
| Workflow drag-in | Drop a JSON or PNG onto the canvas [D] | **Missing:** no dropped-file handling anywhere [S] | S for drop; L with import | |
| Node search | Double-click, aliases, slot filters [S, D] | **Have basic:** Tab or Space, words, 150 results [S] | S to M | Add `search_aliases`, type filters, recents |
| Pack install UX | Manager, Registry, missing-node install [D] | **Missing:** manifests only [S] | M to delegate | Show "missing nodes" and link to ComfyUI |
| Missing models | `models` list with URLs in workflow files [D] | **Missing** | M | Pack command |
| Mask editor | Paint on the image, GPU rendered [D] | **Missing** [S]. The maintainer's idea: PhotoCraft, `ROADMAP.md` [S] | M to L | Open from a node, return a mask as ARRAY |
| Image compare | Node and widget [S] | **Missing** | M | The research software supporting www.virtimp.com has Compare |
| App Mode | Pick inputs and outputs, run a simple page [D] | **Missing** | L | Promoted widgets are a start; deprioritise |
| Video viewer | Yes | **Missing** [S] | L | |
| 3D viewer | Yes | **Missing** [S] | L | |

**A plain fact:** none of these is architectural. PlumCraft's canvas, commands and viewers are in place. Each is ordinary feature work, and it adds up.

## 5. What PlumCraft already does better

Only where there is evidence.

- **Engines are interchangeable, and a graph can span them.** A coordinator splits a graph across engines, through the same interface every client uses (note 15, `crates/coordinator`) [S]. ComfyUI has one process and one namespace of nodes [S].
- **It reaches beyond generative AI.** Thirteen craft packs, array, audio, OpenCV, text; a real graph can trace with VectorCraft and measure with the array engine (demos in note 15) [S].
- **Crash isolation.** Each engine is a supervised process: restart with backoff, stderr shown, the graph kept, a failed run reported as data (spec 16.3) [S]. ComfyUI runs custom nodes as arbitrary code in its only process, and says so (`SECURITY.md`) [S].
- **Authentication and hostile input.** Tokens, loopback defaults, a same-host proof, a hostile-input sweep, and "never crash" as a rule (AGENTS.md) [S]. ComfyUI trusts anyone who can reach its URL [S]. The bug in section 2.2 shows the rule has teeth only where it is tested.
- **Results are identified by content and survive a restart.** Handles, verified bytes, persisted memo records with a salt that includes code and dependency digests (spec 14.2, 14.5) [S]. ComfyUI's default cache is in memory and its signature has no code digest [S].
- **Sweeps.** Vary nodes with zip and product across axes (spec 7.6 rule 2) [S], against ComfyUI's "repeat the last item" (`execution.py:262`) [S].
- **Any value on any wire.** Hover shows it; pop-outs follow a port without blocking the editor; arrays get statistics, histograms and a values grid; tables and YAML have viewers (note 16) [S]. In ComfyUI a value is seen through a preview node [I].
- **Readable large graphs.** Tidy (layout) and net labels (schematic wires), both as commands [S].
- **Everything is a command.** The UI, CLI, control channel and MCP share one registry, with undo (rule 4) [S].
- **A contract with a test suite.** `docs/interface.md`, JSON Schema, shared vectors, `plumcraft conform` for any engine [S]. ComfyUI's API is documented, but nothing like conformance was found.
- **Native.** No browser, and no Python in the editor [S]. Claims about speed would need measuring, and none were made.

## 6. Recommendation

**Build (a1).** Keep ComfyUI as ComfyUI and let PlumCraft be what it set out to be: an editor whose engines are interchangeable. ComfyUI then becomes the most demanding engine PlumCraft has had, and the work repays itself: live previews, queues and bypass help every slow engine.

**In a separate repository**, `~/workspace/comfy-engine`, generated with `plumcraft-engine new`, like `opencv-engine`. Nothing named ComfyUI goes into this repository (rule 3).

**Avoid:** (c) as a parity route, (a2) until (a1) shows why it is needed, and any byte form for MODEL.

### Stages

| Stage | What | Proves | Effort | Rests on |
|------|--------------------------------------|--------------------------|------------|------------------|
| 0 | Fix the opaque crash in the Python SDK, with a test | Rule 1 holds for values that aren't data; needed anyway | S, under a day | A few call sites, found and reproduced here |
| 1 | **The smallest useful milestone.** The adapter: catalogue from `/object_info`, run seam, a run of the API-format graph in ComfyUI's basic_api_example.py, status, progress, cancel, the output image in the Watched panel | The interface holds a foreign executor: 1,000 nodes in a palette, opaque types, status mapped, the memo out of the way | M, 2 to 4 days | About 800 to 1,500 lines of Python; manifests, generator and hosts exist |
| 2 | Interactive: multi-line text, seed control, searchable combos, thumbs then `node.live` snapshots, `schema.changed`, Open in ComfyUI | It feels like generating, not submitting | M to L, 1 to 2 weeks | Eight items from sections 3 and 4 |
| 3 | Import: API format and PNG text chunks, then workflow JSON; drag-in; pack operations; bypass and mute; notes; frames; subgraphs to groups; reroutes to bends | Existing ComfyUI work opens in PlumCraft | L, 2 to 3 weeks | Section 2.7's traps; each has a test from `blueprints/` |
| 4 | Queue, history, batch count, IMAGE and MASK conversion pairs, watching any IMAGE port, `values.held` | Generation next to the array engine and crafts in one graph; PlumCraft's real differentiator | L, 2 to 3 weeks | Queue model is the unknown |
| 5 | By demand: mask through PhotoCraft, video viewer, 3D viewer, blueprint library, more widgets, Comfy Cloud as a target | Parity where it matters | L each | Separate decisions |

Stages 0 to 4 total about **6 to 10 agent-weeks**, with the maintainer reviewing. Everything except the adapter itself is generic editor and SDK work.

**Stage 1's exit test:** `plumcraft run` and the editor, against a local ComfyUI, produce the same image as ComfyUI's own frontend for the same seed; Cancel stops the sampler; a second run with one changed widget reports upstream `cached`; `plumcraft conform` passes under a whole-graph profile.

**Before Stage 1:**

- **Disk.** Around 13 GB is free. An SDXL checkpoint is around 7 GB [I], and `plumcraft-store` declines publishes under its 8 GiB free-space floor [S]. Use a small model, or keep models on another volume.
- **Hardware.** Run first on the Mac under MPS unless a remote server has a GPU [?].

## 7. Open questions for the maintainer

1. **What is the goal?** Running ComfyUI graphs from PlumCraft, or replacing ComfyUI's frontend for yourself, or something for other people? The last is the only one that makes App Mode, Manager integration and the widget zoo matter.
2. **Which hardware?** Does a remote server have a usable GPU? If not, a Mac on MPS sets what's practical.
3. **Distribution.** Will PlumCraft go public, and would you ship ComfyUI with it? If the engine is a separate download, nothing else needs deciding.
4. **Whole-graph engines.** Is it acceptable for an engine to run graphs its own way, with coarser status than the conformance suite expects? This decides `Engine.run_graph` and the conformance profile.
5. **`values.held`.** Note 18 proposes it for the research software; ComfyUI needs it too. Is it a shared decision?
6. **Queue or supersede?** Should the editor's run model change for every engine, or only when an engine declares it is slow?
7. **Bypass semantics.** ComfyUI's bypass wires same-type inputs through. Is that the right generic rule for all engines?

## 8. What could not be verified

- **ComfyUI wasn't run.** Latency, `/object_info` size with custom nodes, and the exact websocket behaviour under the adapter are unmeasured.
- **The frontend wasn't read.** That `graphToPrompt` flattens subgraphs, drops virtual nodes and applies modes is general knowledge, consistent with the workflow files and docs, not verified in source.
- **Mid-run `preview` records** reaching the canvas comes from reading the reducer and canvas, not running them.
- **Hovering a MODEL wire killing the engine** combines two verified facts; the full path through the editor wasn't run.
- **The share of custom nodes that need JavaScript** is unknown. Many popular packs ship web extensions, so "all custom nodes" means "their Python".
- **The node counts** come from a regex over class declarations and include test and dev nodes.
- **diffusers on MPS with offload**, **candle on Metal**, and **EXR** in PlumCraft's picture viewer: not established.
- **`comfy-aimdo`'s licence** isn't stated in what was checked.
- **Licensing** here is a reading of the FSF's FAQ. It isn't legal advice, and the FSF's view isn't the law.
- **Effort** is estimated, not measured; the real-model stages have the widest error.

## Sources

**ComfyUI repository at `0dbfd49`:** `execution.py`, `server.py`, `main.py`, `nodes.py`, `latent_preview.py`, `protocol.py`, `SECURITY.md`, `README.md`, `requirements.txt`, `manager_requirements.txt`, `default_workflow.json`, `comfy_execution/{caching,graph,graph_utils,progress,server_protocol}.py`, `comfy_api/latest/_io.py`, `comfy/{model_management,model_patcher,samplers,supported_models,cli_args}.py`, `comfy_extras/{nodes_logic,nodes_loop}.py`, `blueprints/`.

**PlumCraft:** `AGENTS.md`, `docs/STATUS.md`, `docs/interface.md`, `docs/plan.md`, `ROADMAP.md`, notes 15, 16, 17, 18, `crates/{proto,engine,coordinator,session,ui,graph,store}`, `python/src/plumcraft_engine`.

**Web:**

- [ComfyUI Server Messages](https://docs.comfy.org/development/comfyui-server/comms_messages)
- [ComfyUI Server Routes](https://docs.comfy.org/development/comfyui-server/comms_routes)
- [Workflow JSON](https://docs.comfy.org/specs/workflow_json)
- [V3 Migration](https://docs.comfy.org/custom-nodes/v3_migration)
- [Subgraph](https://docs.comfy.org/interface/features/subgraph)
- [Subgraph blueprints](https://docs.comfy.org/custom-nodes/subgraph_blueprints)
- [Interface Overview](https://docs.comfy.org/interface/overview)
- [App Mode](https://docs.comfy.org/interface/app-mode)
- [Assets](https://docs.comfy.org/interface/assets)
- [Inpainting tutorial](https://docs.comfy.org/tutorials/basic/inpaint)
- [Cloud API Reference](https://docs.comfy.org/development/cloud/api-reference)
- [Registry pyproject.toml](https://docs.comfy.org/registry/specifications)
- [comfy-aimdo on PyPI](https://pypi.org/project/comfy-aimdo/)
- [Comfy-Org repositories](https://github.com/orgs/Comfy-Org/repositories)
- [Licences discussion #14346](https://github.com/Comfy-Org/ComfyUI/discussions/14346) (no maintainer reply; a user's view)
- [GPL FAQ](https://www.gnu.org/licenses/gpl-faq.en.html)
- [candle Stable Diffusion example](https://docs.rs/crate/candle-examples/latest/source/examples/stable-diffusion/README.md)
- [candle Flux example](https://docs.rs/crate/candle-examples/latest/source/examples/flux/README.md)
- [diffusers memory guide](https://huggingface.co/docs/diffusers/en/optimization/memory)
- [diffusers Flux pipelines](https://huggingface.co/docs/diffusers/en/api/pipelines/flux)
- [diffusers issue #9047](https://github.com/huggingface/diffusers/issues/9047)
- Licences of the listed GitHub projects from the GitHub API, 2026-10-31.
