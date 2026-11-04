# Changelog

## 0.2.0 (2026-11-04)

- **Craft nodes take every parameter their command does.** A curated node's leftover parameters are now advanced widgets, inferred from the craft's own description of the command. Unset widgets aren't sent, so existing workflows run and cache as before. The catalogue check can now read FilmCraft's, WordCraft's, PhotoCraft's and SolidCraft's descriptions, where before it silently checked nothing. A pack can hide a key (`hide`) or turn this off (`auto = false`, `[auto_widgets]`).
- **Export nodes take their own format's options.**
  - FilmCraft's MOV, GIF and WAV exports take a time range, as MP4 did; before, they always exported the whole sequence. Its exports also take size, frame rate, loudness and, for MOV, audio and the ProRes profile.
  - DeckCraft exports choose slides.
  - SolidCraft's STL, 3MF and OBJ take a tessellation quality.
  - DesignCraft, VectorCraft, FigCraft, GridCraft, EffectCraft, SoundCraft, WordCraft and PdfCraft exports gain their formats' options too.
  - GridCraft's CSV export now runs `file.exportCsv` (`sheet`, `values`), and WordCraft's PNG export runs `file.exportPng` (`page`, `scale`, `markup`).
- **Node families generated from a craft's command list** (`[[generate_commands]]`): 132 PhotoCraft filters and adjustments, and 98 EffectCraft layer commands. Each is a node with widgets from the command's description; curated nodes win.
- **SolidCraft: Sketch from SVG and Extrude**, so a part can start from a drawn profile.
- **A parameter-coverage test** (`crates/craft/tests/params_coverage.rs`) fails when a craft node starts hiding a parameter. To accept a new baseline, run it with `PLUMCRAFT_BLESS=1`.
- **A craft choice can bring its own parameters** (`brings = "<catalogue>:<field>:<list>"` on a widget). SoundCraft's Insert plug-in now shows the chosen plug-in's parameters from `engine.plugins`, each with its default: Compressor/Limiter's Threshold, Ratio, Attack and so on. Only the chosen plug-in's are sent. Choices are shown by name, toggles as checkboxes. Before, it showed an empty `params` object.
- **The editor:**
  - Engine › Pick Engines Automatically (`engine.auto`): opening or inserting a workflow starts the engines it needs, each hosted the fastest way (in-process, then a process, then HTTP). `engine.use` without a `mode` picks the fastest too.
  - A workflow from elsewhere asks to be trusted with just **Trust**, not Trust and Run: trusting it no longer runs it. The held run is dropped, and it runs at the next run.
  - A socket's label names its type when its name doesn't (`audio · WAV`), and every port's tooltip starts with its type.
  - A double-click on a node opens Edit Parameters, and the window opens beside the node.
- `plumcraft-editor run FILE` runs a workflow's Write Files nodes (any `output_node`) when it has no Result node, so a graph of craft, array or audio nodes runs headless as it does in the window. `--out DIR` points their folders there. Before, such a run stopped with "this engine doesn't save files".
- `run`, `check` and `tidy` start the engines the workflow names in its `engines` list when no `--engine` is given.
- `plumcraft-editor check FILE` reports unknown node types and inputs, wires the type rules refuse, wire-only inputs left unwired, and nodes no output depends on, without running anything.
- `plumcraft-editor tidy FILE` lays a workflow out as Edit › Tidy does and saves it.
- Popping out a craft document that PlumCraft has no view of its own for (every craft's but PdfCraft's) opens it in the craft's app, as Open in … does. Save it there and the saved document goes to the nodes downstream in place of the node's own, and the graph runs again. Saving the original back, or `doc.revert`, returns to the node's own value. Reopening opens the edited copy. Edits last while the graph is open and aren't saved with the workflow.
- Open in … works on small documents. A document under the 64 KiB inline limit (a SoundCraft session, say) used to be refused as being "on a remote engine".
- `run`, `check` and `tidy` given `--engine-manifest` but no `--engine` still start every engine the workflow names. Before, a manifest switched that off and the run stopped asking for `--engine`.
- GridCraft's Export PDF (`gc.export_pdf`) can export every sheet: `sheets: all`. It now runs GridCraft's own PDF export, which honours each sheet's Page Setup; `active`, the default, gives the same pages as before.
- A `multiline` widget hint: a STRING port with it gets a monospace text box, a few lines on the node and more in the parameter editor, scrolling past that. Enter is a new line; a click elsewhere commits. The Python SDK takes it as `Port(widget="multiline")`.
- A shortcut pressed while typing in a text field (⌘↩ Run, say) waits for the field to commit, so it acts on what was typed. Before, a run started from a field used the old value.
- Craft packs can declare file sockets (`[[node.file]]`): a value wired in is staged in the craft's work folder and its path sent to the command. PdfCraft's Open takes a wired PDF, a new Insert pages node puts another PDF's pages in, and Header and footer gained left and right text, colour, font, pages and margins.
- **Convert** (`docs/research/20-convert.md`). Engines mark the nodes that turn one type into another (`converts` on a NodeDef; `@node(converts=True)` in Python). WAV to Audio and Audio to WAV, the new Audio to Array and Array to Audio, every craft export, and DuckDB's Table to Rows, Rows to Table and Table to CSV are marked.
  - A wire the types refuse offers the converters that bridge it beside the error, "Insert WAV to Audio". `wire.convert` places and wires them as one undo step, and `plumcraft-editor check` names the fix.
  - The new core node, Convert, takes its types from its wires and keeps the path it picks in the workflow. Before a run it is replaced by its converters, which may be on several engines, and their results come back as the Convert's. A SoundCraft session wired through a Convert into an Audio input runs as Bounce WAV, then WAV to Audio.
- The audio engine has Audio to Array (`audio.to_array`) and its inverse, Array to Audio (`audio.from_array`). Audio to Array gives the samples as a (frames, channels) float32 array, its columns named, with the sample rate as a second output. Array to Audio takes (frames, channels), or 1-D for mono, and a sample rate, 48 kHz unless wired. So a WAV, or a SoundCraft session, reaches the array engine through a Convert.
- Engines ship as Python packages. Every engine's console script has `register`, which writes a manifest into `~/.config/plumcraft/engines/` that starts the installed engine with its own Python, and `unregister`, which removes it. A generated project's wheel carries its `examples/`, so the demo comes along. So `uv tool install my-engine` then `my-engine register` offers it in every editor. A checkout's `scripts/install` link is left alone unless `--force`.
- Converters between core types, in the text engine (Rust and Python alike), each with its inverse: Text to Table and back (the text as CSV), JSON to Text and back, and Table to Array and back. The text engine declares JSON.
- Video as arrays, in the array engine, with ffmpeg: MP4, MOV and GIF to Array give a clip's frames as one (T, H, W, 3) array (or float gray) and its frame rate; Array to MP4 (H.264), Array to MOV (ProRes) and Array to GIF are their inverses. A clip over 512 MiB of frames is refused whole. FilmCraft's exports now reach the array engine through a Convert node.
- A mark's ARRAY end may also be `video`, (T, H, W, C). The Python SDK says a mark's shape: `@node(converts=True, shape="series")`.
- File decoders, each with its inverse, all converters:
  - **Text, in Rust and Python alike:** TXT, Markdown and HTML to Text and back, and CSV to Table and back.
  - **Audio:** FLAC and AIFF to Audio and back.
  - **Array:** Rasterize PDF (a page as a picture) and Array to PDF.

  Craft exports in those formats now reach other engines through a Convert node.
- More converters marked: Decode Image, Encode Image and Rasterize SVG, and every craft's Open (from a picture, an SVG or a PDF). A node may carry several marks.
- An ARRAY end of a mark may say its shape, `image` or `series`, and a path never crosses them. A sound never becomes a picture through Convert, or the reverse.
- The array engine keeps a 1-D array 1-D. Its nodes used to give back a 1-D input as a 1 × n array.
- `plumcraft-editor check` says each problem once. A refused wire was said twice.
- Engine › Install Engine… (`engine.install {repo}`) installs a Python engine from GitHub, named as `owner/repo` or by its URL. The window offers the public engines (DuckDB, OpenCV, Atlas). It runs `uv tool install` and the engine's `register` in the background, with the status bar saying so, then reloads the Engine menu.
- The README lists the public engine repositories, `plumcraft-<name>-engine`.
- Rust engines have `register` and `unregister` too. `plumcraft_engine::serve::main` is the whole of an engine binary's `main`: register, unregister and `--help`, then serving. A craft pack kept in a file registers with `plumcraft-craft-engine --craft PACK.toml register`. PlumCraft's own engines say they need neither.
- The Engine menu puts its commands (Reconnect, Attach…, Reload Engine List, Clear Engines) at the top, and lists the engines below them in a list that scrolls. Each engine is a submenu of its connections (in-process, process, Python, HTTP), with the one in use named beside it. Picking a connection adds that engine beside the others, or changes its connection when it is connected (`engine.use`); there is no Add Engine submenu. Clear Engines (`engine.clear`) stops them all, so a complete change is a clear and a pick. Before, every engine and every connection was one long list that ran off a short window, and a pick replaced every engine.
- `plumcraft-engine check`, and the tests of a newly generated engine, make the demo's relative paths absolute against its folder, as `plumcraft-engine run` and the editor do. A demo that reads a file beside it used to fail conformance.

## Installers 0.1.0, 2026-11-02

[Installers](https://github.com/seantrue/plumcraft/releases/tag/app-v0.1.0) for macOS on Apple silicon and x86-64 Linux, with every engine, audio included.

## plumcraft-engine 0.1.1, 2026-11-02

Generated engines now say how to get the Rust tools, `cargo install plumcraft plumcraft-editor`. 0.1.0's message named a package that was renamed.

## 0.1.0, 2026-11-02

The first public version. The Python SDK, `plumcraft-engine`, is on [PyPI](https://pypi.org/project/plumcraft-engine/). The editor is [`plumcraft-editor`](https://crates.io/crates/plumcraft-editor) and the command line [`plumcraft`](https://crates.io/crates/plumcraft) on crates.io. The audio engine builds from a checkout, and `plumcraft run --python` needs one too.

- A native node-flow editor with interchangeable engines, speaking protocol 2 in-process, over a Unix socket or over HTTP.
- Engines for text (Rust and Python), audio and arrays, plus craft apps through their MCP servers.
- Several engines in one graph, with values shared by content handle.
- A Python engine SDK, and `plumcraft-engine new` to generate an engine project.
- `plumcraft-engine new` works from an installed package: `--sdk pypi`, the default outside a checkout.
- `zip` 9, for three advisories against 3.0. A bundle entry named from a root or a drive is refused.

See `docs/guide.md` for what's built and `docs/STATUS.md` for current work.
