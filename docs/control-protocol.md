# Control protocol

*Last updated 2026-10-29 (several engines; before that E1.4, engine switching). Lifted from the research software's `docs/control-protocol.md` (the research software supporting www.virtimp.com `8d96021`), itself modelled on photocraft's.*

The editor (`plumcraft-editor`) serves a JSON control channel on the loopback interface, so agents, tests and scripts can drive the real window: run any session command, read the UI state, wait for runs and take screenshots.

## Starting it

```sh
plumcraft-editor --control 7878 --control-token-file ~/.config/plumcraft/control.token
```

- The server listens on `127.0.0.1:<port>` only. Port `0` picks a free port; the editor prints the one it bound to stderr (`plumcraft-editor: control channel on 127.0.0.1:<port>`).
- **Token:** with `--control-token-file`, the editor reads the token from that file, or creates the file (mode `0600`) with a fresh 256-bit token when it doesn't exist. It refuses a file that is a link, belongs to someone else, or that others can reach. Without it, a token is generated for this launch and printed to stderr (`plumcraft-editor: control token: …`). `plumcraft_automation::server::default_token_file()` names a conventional place, `~/.config/plumcraft/control.token`.
- The token is 64 hexadecimal characters.
- Other launch options: a workflow file to open (the text engine's demo without one), the engine and its hosting (`--engine text|audio`, `--process`, `--python`, `--http [--python]`; the Engine menu, or `engine.switch`, changes them while the editor runs) and `--screenshot OUT.png [--shot-runs N]`.

The transport is `crates/automation/src/server.rs`. The handlers are `crates/ui/src/control.rs`; the editor app (`EditorApp`) drains the transport's queue at the start of every frame, and `apps/plumcraft-editor/src/main.rs` starts the server.

## Framing and authentication

- One JSON object per line in each direction (newline-delimited JSON). A request line may be at most 1 MiB.
- **The first frame on every connection must authenticate.** No other method runs before it succeeds:

  ```json
  {"id": "auth", "method": "auth", "params": {"token": "<64 hex characters>"}}
  ```

  A wrong token or any other first frame gets `{"ok": false, "error": "authentication required"}` and the connection is closed. A connection that doesn't authenticate within 10 seconds is closed. A second `auth` on an authenticated connection gets `"already authenticated"`, and the connection keeps serving.
- **Requests:** `{"id": <any JSON>, "method": "<name>", "params": {…}}`.
- **Replies** carry the request's `id`:

  ```json
  {"id": 1, "ok": true, "result": {…}}
  {"id": 2, "ok": false, "error": "unknown method \"foo\""}
  ```

  A line that isn't JSON gets `{"id": null, "ok": false, "error": "bad JSON: …"}`, and the connection keeps serving.
- **Timing:** requests are queued to the UI thread and handled at the start of the next frame (the server wakes the window). The server waits up to **30 seconds** for a reply; after that it answers with an error starting `timeout:`. A timeout doesn't undo work that already ran. Every request gets exactly one reply.
- At most 16 connections are served at once.

## Methods

| Method | Params | Result |
|---|---|---|
| `command.run` | `{command, params?}` | The command's result. Runs any session command by id (`node.add`, `node.set`, `wire.connect`, `run.start`, `run.watch`, `graph.new`, …) exactly as a menu or shortcut would, journaled, with the same side effects in the window (`graph.new` fits the view; `run.watch` reruns the graph so the Watched panel fills). A failure is also shown in the status bar. `engine.execute` is an alias (the research software's name). |
| `command.list` | none | Every command: `{id, label, menu, shortcut, params, enabled, disabledReason}`. `engine.commands` is an alias. |
| `graph.get` | none | The graph as its workflow JSON (`{version, nodes, wires, next_id, meta?}`). |
| `ui.inspect` | none | The UI state, below. |
| `ui.invoke` | `{command}` | The UI state (as `ui.inspect`) plus `invoked`. Runs a command **as its menu item or shortcut does**, with what the UI fills in: the canvas selection, the native file dialogs (`graph.open`, `graph.save` of an unsaved graph, `graph.saveAs`: the dialog blocks the window until it is answered, so a control client gets its reply then), and the unsaved-changes prompt (`graph.new`, `graph.open` or `app.quit` over unsaved changes opens it instead: answer it with `ui.prompt`). `ui.invoke frame.add` frames the selection, as Edit › Frame Selection does. `ui.invoke graph.tidy` lays out the selection (two or more nodes) or everything with the node sizes and pin positions the canvas measured, then fits the view; `command.run graph.tidy` estimates them unless `sizes` and `pins` are passed. `ui.invoke app.quit` closes the window as its close button does (the app exits; the reply comes first), so a client can drive quitting through the prompt. A disabled command is an error with the reason. Use `command.run` for the same command with explicit parameters. |
| `ui.prompt` | `{answer: "save" \| "discard" \| "trust" \| "cancel"}` | The UI state. Answers the open prompt (`ui.inspect`'s `prompt`) as its buttons do: `discard` goes on (New makes the graph; Open shows the dialog, then opens with `discard: true`; Quit closes the window), `save` saves first (the save dialog for a graph with no file) and goes on once nothing is unsaved, `cancel` closes it. An error when no prompt is open. |
| `ui.set` | `{zoom?, pan?, fit?, palette?, focus?}` | The new canvas state. `focus`: a group id enters that group (only its members are shown), null leaves it; an unknown group is an error. `zoom` is the canvas scale (0.05–8), applied about the canvas centre. `pan` is the translation `[x, y]` in screen points (graph → screen is `screen = graph · zoom + pan`). `fit: true` fits all nodes, as View › Fit. `palette`: `true` opens the node palette, `false` closes it, a string opens it with that search text. Unknown fields are an error. |
| `ui.audio` | `{play?, loop?}` | The audio view's state (below). `play: true` plays the watched audio from the start through the system output, `false` stops; `loop` loops it or not. Playback lives in the UI, not the session (it is not journaled). With nothing to play, or no output device, `play: true` is an error (`no audio output`) and nothing else changes. Unknown fields are an error. |
| `ui.waitRun` | `{timeout?}` | Replies when no run is in flight or pending (an auto-run waiting out its 150 ms, a request not yet sent), with the `run` object of `ui.inspect`. `timeout` is in seconds (default 25, at most 29, under the 30 s reply limit); past it the reply is an error starting `timeout:`. Use it after `command.run run.start`, `run.watch` or an edit with auto-run on. |
| `ui.screenshot` | `{path?, offscreen?}` | `{path, width, height, source}`: a PNG of the window. The window is asked with egui's `ViewportCommand::Screenshot` a few frames after the request (so layout settles); `source` is `"window"`. **If the window doesn't deliver within 2 s** (macOS never does while the screen is locked), the editor draws itself offscreen at the window's size with the wgpu renderer egui_kittest uses, and `source` is `"offscreen"`. `offscreen: true` skips the window. Without `path`, the file goes to `$TMPDIR/plumcraft-screenshots/plumcraft-<pid>-<n>.png`. An offscreen picture shows the same graph and view; thumbnails are read again for it. |

`plumcraft_automation::dispatch` handles `command.*` and `graph.get` against a bare session (the headless form, without a window).

### `ui.inspect`

```json
{
  "title": "flow.json • — PlumCraft",
  "document": "flow.json",
  "path": "/Users/me/flows/flow.json",
  "dirty": true,
  "undo": "Connect",
  "redo": null,
  "prompt": null,
  "copied": false,
  "closing": false,
  "graph": {"dirty": true, "nodes": [{"id": "n2", "kind": "text.upper", "title": "Upper case", "pos": [320, 0], "view": "full"}], "wires": 1, "generation": 8},
  "selection": ["n2"],
  "canvas": {"zoom": 1.0, "pan": [0, 0], "rect": [8, 31, 972, 729], "editing": false},
  "engine": {"label": "engine: Text 0.1.0 · Python · mode 2 · process · connected · 13 nodes", "state": {"state": "connected", "version": "0.1.0", "nodes": 13, "caps": ["…"]}, "connected": true,
             "engine": "text", "mode": "python", "hosting": "Text · Python · process (mode 2)", "pending": false,
             "choices": [{"engine": "text", "mode": "inproc", "engineLabel": "Text", "modeLabel": "Rust · in-process (mode 1)"}, "…"]},
  "banner": null,
  "catalog": {"nodes": 13, "engine": "Text", "version": "0.1.0"},
  "popups": {"palette": {"query": "upper", "selected": 0, "at": [640, 400]}, "anyPopup": false},
  "lastError": null,
  "run": {"running": false, "progress": [2, 2], "nodes": {"n2": {"state": "done", "ms": 0.02, "thumb": true, "stale": false}}, "…": "…"},
  "watched": {"node": "n2", "port": "text", "summary": "text, 26 characters, 1 lines", "full": false, "missing": false},
  "theme": "medium",
  "groups": {"focus": null, "selected": ["g1"],
             "list": [{"id": "g1", "name": "think it over", "collapsed": true, "parent": null, "nodes": ["n2", "n3"], "pos": [320, 210],
                       "promoted": [{"label": "delay", "port": {"node": "n2", "port": "seconds"}, "doc": "", "min": null, "max": null}],
                       "outputs": [{"name": "shout", "from": {"node": "n3", "port": "text"}}]}],
             "proxies": [{"group": "g1", "inputs": [{"label": "delay", "node": "n2", "port": "seconds"}, {"label": "Slow › text", "node": "n2", "port": "text"}],
                          "outputs": [{"label": "shout", "from": {"node": "n3", "port": "text"}}]}]},
  "frames": [{"id": "f1", "title": "inputs", "pos": [40, -60], "size": [480, 320], "color": null, "nodes": ["n1"], "groups": []}],
  "rename": null
}
```

- `document` is the window title's name: the workflow's file name, or `Untitled` (or `Demo`) without one; `path` is the workflow file (null until it is saved or opened); `dirty` is true while it has unsaved changes (also `graph.dirty`, as before).
- `undo` and `redo` are the labels of the steps `edit.undo` and `edit.redo` would take (`Add Node`, `Set Value`, `Connect`, `Paste Nodes`, …), or null.
- `prompt` is the question the window is asking, or null: `{kind: "unsavedChanges", text, answers: ["save", "discard", "cancel"]}` or `{kind: "reviewWorkflow" | "allowUploads", text, answers: ["trust", "cancel"]}` (answer it with `ui.prompt`). While it is open the keyboard shortcuts wait. Closing the window over unsaved changes (its close button, Cmd+Q, `app.quit`) cancels the close and opens it.
- `closing` is true once the window is free to close (changes discarded or saved, or nothing unsaved when it was asked to); `--screenshot` closes without asking.
- `copied` is true once Copy or Cut has put nodes on the clipboard.
- **A workflow from elsewhere over the channel.** A run of a graph read from a file nobody approved is held: `command.run run.start` replies with `held`, `ui.inspect`'s `run.awaitingReview` is true, and the window asks. `graph.review` says what the graph touches. `graph.trust`, or `ui.prompt {answer: "trust"}`, approves the graph and remembers it by its hash. The held run is dropped, not sent: the graph runs at the next `run.start`. The token carries the owner's authority, so an agent may approve; read the review first.
- **Unsaved changes over the channel.** `command.run graph.new` and `command.run graph.open {path}` refuse to drop unsaved changes (`the graph has unsaved changes: save it first, or pass discard: true`); pass `discard: true` to go on, or use `ui.invoke` and `ui.prompt` to go through the prompt as a person would.
- `run`: `generation`, `runId`, `running`, `auto`, `autoPending`, `progress` `[finished, total]`, `elapsedMs` (since the latest request), `lastRunMs` (request to reply), `engineMs` (the engine's own `elapsed_ms`), `firstStatusMs`, `errors`, `runError` (a run-level failure such as "the engine went away"), `superseded` (runs replaced while in flight), `watch` (`[{node, port, summary}]`), and `nodes`: `{id: {state, ms, thumb, stale, staleWhy?, error?}}` with state `queued`, `started`, `progress`, `preview`, `done`, `cached`, `error`, `skipped` or `cancelled`, or null (not run).
- `watched`: what the Watched panel shows (null when nothing is watched): the node and port, the display's summary, whether its full value was read (`full`) and whether its bytes were gone (`missing`).
- `watched.audio` (an audio value only; else null): `{clip, error, spectrogram, playback}`. `clip` is `{rate, channels, frames, seconds}` once the full value is read; `spectrogram` is null, `{state: "pending"}`, `{state: "ready", frames, bins, fft, hop, peakHz}` or `{state: "failed", error}`; `playback` is `{playing, loop, position, output, error}` with `position` in seconds and `output` `{name, rate}`, `{error}` (`no audio output`) or null (not probed yet).
- `watched.media` is the display's media kind (`image`, `vector`, `doc`, `audio`, `text`, …); `watched.view` (M3) is the picture viewer's state for `image`, `vector` and `doc` (null otherwise). Image: `{kind: "image", state, size, texture, error, zoom}` with state `none`, `decoding`, `ready` or `rendition` (the engine's rgba8 rendition is shown: a format other than PNG, or the bytes are gone). SVG: `{kind: "svg", state, size, raster, renders, error, zoom}` with state `none`, `drawing`, `ready` or `fallback` (it didn't parse, or is over 64 MB: the rendition is shown and `error` says why); `raster` is the size of the picture resvg drew last, `renders` how many times it drew this value (it draws again, sharper, when zoomed in). Doc: `{kind: "doc", rendition, open, zoom}` where `open` is the "Open in …" button: `{label, enabled, reason, note}` (`reason` why it is disabled, such as a remote engine's document; `note` where the last copy went, or why the last open failed). `zoom` is relative to fitting the view.
- `docs` (M3): the apps that open documents and the latest open. `apps`: `[{app, wire, label, program, missing}]` (`missing` says why the program isn't there, with the fix); `root` is where copies go (`~/Library/Caches/plumcraft/open` on macOS); `pending` the `{node, port}` an open is waiting for; `last` `{ok: true, opened: {node, port, app, path, handle, reused}}` or `{ok: false, error}`.
- **Edits made in the app flow downstream.** Every copy `doc.open` starts an app on is watched (size and modification time, every 0.5 s). A save that holds still for one look is hashed, published to the store, and bound in place of `node.port`'s value on every input wired from it (`RunRequest.inputs`); those nodes and everything downstream go stale with `staleWhy` "`node.port` was edited in `<App>`", and the graph runs (`run.start`). The node itself keeps its own value. Saving the original bytes back drops the edit; `doc.open` on an edited output opens the edited copy again (`reused: true`, the edit's handle). `command.run doc.revert {node, port?}` drops an edit, keeps the file, and runs. `ui.inspect`'s `docs.edits` lists the copies watched (`{node, port, app, path, original, digest, edited}`) and `last`, the latest save taken back (`{ok, applied: {node, port, path, handle, edited, run_id}}` or `{ok: false, error}`). Edits go with the graph (`graph.new`, `graph.open`) and with their node, and aren't saved in the workflow.
- **Opening a document in its app.** `command.run doc.open {node?, port?}` (default: the watched node, its first document output) copies the node's document value, checked against its `md5:raw:` handle, to `<root>/<node>/<title>-<digest8>.<ext>` and starts the craft's desktop app on the copy (VectorCraft: `vectorcraft <file>`; the program comes from the craft manifest's `[gui]`, or `PLUMCRAFT_VECTORCRAFT_APP`). The engine's own CAS file is never opened. An unchanged copy is reused (`reused: true`); one changed in the app is kept and the next open gets `-2`, `-3`, …. When the node has no fresh value (not run, stale, or not watched or targeted), the reply is `{pending: true, node, port, app, run_id}`: the output is watched and the graph run (memo hits make it quick), and the copy opens when the value arrives; wait with `ui.waitRun`, then read `docs.last`. A small document carried inline (under `limits.max_inline`) is copied from its inline bytes, checked the same way. Errors: a document on a remote engine (no bytes on this machine), a file that no longer holds its handle, a missing app (with the fix), no engine connected and no value.
- `engine.engine` and `engine.mode` name the engine the editor has, or last tried (`engine.switch`'s ids; `mode` is `attach`, with `url`, after `engine.attach`; the token is never reported). `choices` lists what the Engine menu offers (`engine.list` returns the same). `hosting` is how the status bar and window title name it.
- **Switching engines.** `command.run engine.switch {engine, mode}` (engines `text`, `audio`, `vectorcraft`; modes `inproc`, `process`, `python`, `http`, `http-python`; only the text engine has a Python twin, and audio is never `inproc`), `engine.reconnect` and `engine.attach {url, token?}` switch before they reply: the run in flight is cancelled, the old engine stopped, the new one started and its catalogue taken. The result is `{engine, mode, url?, connected, label, problems}`, where `problems` lists the nodes and wires of the graph the new catalogue lacks (the graph is kept). A switch that can't connect is an error with the reason (also in the status bar); the editor then has no engine and keeps the old catalogue.
- **Engines from manifests** (spec 16.6). `--engine-manifest FILE` (repeatable) and every `~/.config/plumcraft/engines/*.toml` add to the Engine menu and `engine.list`: an engine with a `command` as `process` and `http`, one with a `url` as `http`. With `--engine-manifest` and no `--engine`, the first manifest's engine opens, on its `demo`. An id that is a built-in engine's, or another manifest's, is refused, naming both files, and the rest still load. `command.run engine.manifests.reload` (Engine › Reload Engine List) reads them again and returns `{engines, problems}`; the first problem also goes to the status bar. An engine a manifest stops naming stays connected until it is switched away from.
- **The Engine menu's pick.** `command.run engine.use {engine, mode}` is what picking a connection in the Engine menu does: with nothing connected it starts that engine (`how: "start"`, as `engine.switch`); an engine not connected is added beside the others (`"add"`, as `engine.add`); one connected another way changes its connection and keeps the others (`"change"`); the connection in use already does nothing (`"unchanged"`). It replies after every step, with the last step's result and `how`. `engine.clear` (Engine › Clear Engines) stops every engine and replies `{engines: [], label}`: the graph stays, on the catalogue it had, with none connected, so a complete change is a clear and a pick.
- **Installing an engine.** `command.run engine.install {repo}` takes `owner/repo` or `https://github.com/owner/repo` and replies `{installing: url}` at once. The editor then runs `uv tool install --reinstall git+URL` and `register` on each command it installed, off the UI thread, and reads the engine manifests again. `ui.inspect`'s `engine.installing` is the URL until then, and the status bar says how it went. One install runs at a time, and it runs the package's own code.
- **Several engines** (`docs/research/15-multi-engine.md`). `command.run engine.add {engine, mode}` starts another engine beside the ones there are (an engine already connected is an error); `engine.remove {engine}` stops an added one (the first is switched, not removed). Both relink before they reply, as a switch does (the run in flight is cancelled, the results forgotten, the graph kept), and return `{engines: [id], connected, label, problems, warnings}` plus `added` or `removed` (`warnings`: what merging the catalogues found wrong, such as a wire type two engines declare differently; also `engine.warnings`, and the status bar when there are no problems). With engines added, the editor talks to a coordinator: one merged catalogue, whose nodes name their engine (`NodeDef.engine`), and runs that cross engines. `engine.engines` lists every engine with its `state` (`[{id, state}]`; one entry without added engines), `engine.list` adds `added`, Switch and Attach go back to one engine, and Reconnect restarts the first and adds the others again. Starting with several: `--engine text,array` (the others hosted as the first where they can be). **Which engine runs a node:** a node whose kind several engines offer is pinned to the first when placed (the workflow's `engine` field), and the first placement writes the workflow's `engines` order; `command.run node.engine {nodes, engine | null}` pins or unpins (an undo step; the nodes go stale). The canvas shows each node's engine in its header (`•` when pinned), the node menu has a Run on row, and Node › Add has a menu per engine (kinds several offer under Shared).
- **Viewers and pop-outs** (`docs/research/16-viewers.md`).
  - `command.run view.popout {node, port?, index?, viewer?}` opens a window that follows the port's value: `{id, …}`, or `{id, existing: true}` when that port is already open. It updates with every run and never blocks the editor; native windows where the platform has them.
  - **A craft document PlumCraft has no view of its own for** (the `[gui]` app's documents, except where a viewer other than the rendition shows them: PdfCraft's pages) opens in its app instead, exactly as `doc.open` does: the reply is `doc.open`'s plus `in_app: "<label>"`, and no window opens. `window: true` asks for PlumCraft's window anyway. Saves in the app flow downstream (see `doc.revert` below).
  - `view.close {id}` closes one, `view.list` lists them, and `view.viewer {viewer, popout?}` picks the panel's (or a pop-out's) viewer.
  - `view.peek {node, port?}` asks for a port's value for a wire's hover without changing the panel (`{}` drops it).
  - `run.view` and `run.item` take `popout` to act on a pop-out's value.
  - **The viewer's tools** act on the panel, or with `popout` on a pop-out; `viewer.pin`, `viewer.unpin` and `viewer.compare` are the panel's.
    - `viewer.probe {row, col, popout?}` pins the pixel probe at an array pixel; `{}` unpins. `ui.inspect`'s `watched.view.tools.probe` reads it: `{row, col, text, dtype, names, values}` with each value as exact text (JSON has no NaN), `{outside: [rows, cols]}` past the array, `{reading: true}` while an image decodes, or `{unavailable: why}` when the numbers can't be read here. No number is ever made from the 8-bit picture. `hover` is the same for the pixel under the pointer.
    - `viewer.zoom {to?, at?: [row, col], popout?}`: `to` is `"fit"`, `"1:1"`, `"in"`, `"out"`, `"detail"` or screen pixels per array pixel (1 to 64), `at` centres a pixel. Enlarged, a picture's pixels are nearest-neighbour blocks. When the array is larger than the picture sent, it raises the watch's `px` to the array's long side (at most 4096) and returns the `run_id` of the cached run that brings a picture that size; Fit lowers `px` for the next run. `detail` asks for that without moving the view, as scrolling in does. `watched.view.tools` has `pixelsPerArrayPixel`, `arrayPixelsPerBlock` (over 1: this engine sent a smaller picture) and `pictures` (screen rects); `watched.px` is the watch's size.
    - `viewer.pin {slot?: "a" | "b"}` keeps the panel's value as A or B and turns the compare on; a side not pinned is the panel's value. `viewer.unpin {slot?}`. `viewer.compare {on?, how?: "side" | "split" | "difference", diff?: "mean" | "channels", split?: 0..1, channel?}`. `watched.compare` (null when off) has `how`, `diff`, `split`, `channel`, `a` and `b` (`title`, `summary`, `full`, `missing`, `pictures`), `probe` and `hover` (`{a, b, difference}`), and `result`: `{state: "computing"}`, `{state: "error", error}`, or `{state: "ready", byHandle, text, max, mean, nan, differing, elements, pictures: [{caption, size, top}]}`. Two equal handles are the same bytes: `byHandle` is true and nothing is read.
    - `viewer.sheet {on?, index?, popout?}` shows a list's items as a labelled grid (the `items` viewer) and asks for their pictures; `index` shows that item as it closes. `watched.sheet` says whether it is on.
  - **Viewer ids:** `picture`, `svg`, `pdf.pages`, `mesh` (STL files, bytes typed `model/stl`, binary or ASCII: shaded, turned by a drag, zoomed by scrolling; its `state` reports `triangles`, `ascii`, `bounds`, `dropped` and the `camera`), `doc`, `array.plot` (an (N, k) table of positions), `array.pictures`, `array.grid` (values, PyCharm-style), `audio`, `table.grid`, `xml` (XML text, or bytes typed `application/xml`, `text/xml` or `…+xml`, as an outline; ahead of `text.plain`), `text.plain`, `scalar`, `yaml`, `tree`, `items`, `hex`.
  - **`ui.inspect`:** `popouts` lists each pop-out with its pane (`summary`, `viewer`, `viewers` offered, the viewer's `state`).
  - **Value editors:** `command.run node.editor {node, open?}` opens and closes the editor a node names (`docs/interface.md` 7.4). `ui.inspect`'s `editors` lists those open: `{node, kind, state}`, `state` being whatever that editor reports.
  - **Wires:** hovering one shows its value's preview; a double click pops it out.
- `engine.state` is the host's `EngineState` (`docs/interface.md`): `off`, `connecting`, `unreachable` (with `url`, `reason`, `hint`), `engine_down` (with `reason`, `hint`, `stderr_tail`), `loading_schema`, `unavailable`, `busy` or `connected`.
- `banner` is the text of the banner shown when the engine is down or can't be reached, including the fix.
- `groups` (E3): `focus` is the group entered (null: the whole graph); `selected` the collapsed groups selected (a click on a group's node selects its group; `selection` lists nodes only); `list` every group with its direct members, parent, promoted inputs and outputs; `proxies` each collapsed group drawn as one node, with what each of its pins stands for (a member's port). Wire to or from a proxy pin with `command.run wire.connect` on the member's port.
- `frames`: each frame as `{id, title, pos, size, color, nodes, groups}`, with what it holds now (the nodes and groups `frame.move` would carry). Frames are edited with `frame.add`, `frame.set`, `frame.remove`, `frame.move` and `frame.resize`.
- `save`: `{pending, last}`. `command.run run.save {node, port?, index?, sheet?, path, options?}` asks the engine to write the value, as the buttons do, and answers at once with `{pending}`; it needs `save_formats` in the catalogue. `last` is `{request, ok, paths, recipe, items, labels, error}` once the engine has replied: poll for `pending: null`. `options` are the format's (`save_formats[].options`), checked; a bare name takes the first format's extension.
- **Bundles.** `command.run bundle.detach {node}` copies the values of the bundle wired into a node (its `flags.bundle_port` input) into its widgets and disconnects it: `{copied, unresolved}`, `unresolved` being what came from wires upstream. `bundle.reset {node, port}` removes a wire that overrides a bundled value.
- **Files an application opens.** `command.run graph.open {path}` on a file an application's opener takes (a picture holding its recipe, in the research software's application) replies with `openedFrom`; the graph opens unsaved, and `path` is null. On any file, `missingEngines: {engines, kinds, text}` says what the catalogue lacks: `text` is also `ui.inspect`'s `banner`, until the engine comes or the graph goes. An engine's `texts.engine_notice` is the banner too.
- **About.** `command.run app.about` opens the About window and replies `{title, version, parts, engine, components}`; `ui.inspect`'s `about` is what it shows while it's open.
- **Export marks.** `command.run group.checkpoint {group, node, on}` marks a member (a node type the catalogue calls `export.markable`) in the group's `checkpoints`. A run ignores it.
- **Expand and bundles.** `command.run node.expand {node, group?}` replaces a command node with the graph the catalogue's `expansions` give it: `{nodes: {local: id}, group, dropped, fromBundle, unresolved, lost}`. `command.run graph.bundle {path}` writes the workflow and the files it reads to a `.zip`: `{path, entries, missing}`; `graph.open` and `graph.insert` take a `.zip`, unpacked into a new folder beside it.
- `viewer.recipe {popout?, copy?}`: what made the value shown, `{recipe}`, null when the engine sent none.
- `prefs`: `{path, theme, dirs}`: this application's preferences file (null: none, they last as long as the window), the theme kept, and the folder each kind of dialog last used. `command.run view.theme {name}` changes the theme (`paper`, `pewter`, `dusk`; `medium` and `dark` are read as Pewter and Dusk), as Settings › Appearance does.
- **Messages.** The status bar shows its messages (the run's errors, a run error, the application's segment, a save's outcome, the last command's error) as chips, the most serious first, each on a background by severity (accent, warning or error, mixed into the panel) in the ordinary text colour. What doesn't fit is cut short, and a `more…` button (`+N more…` when N are left out altogether) says so. `command.run view.messages {open?}` (View › Messages; clicking a message or that button) opens or closes the Messages window, which shows each in full, and replies `{open}`; without `open` it toggles. It doesn't clear the messages it shows.
- **Settings.** `command.run app.settings {tab?: appearance | engines | cache, open?}` opens (or closes) the Settings window and replies with every setting: `{open, tab, theme, themes, appearance: {textSize, zoom, overrides, fruits, families}, contrast: {pairs, below}, autoEngines, cacheReport}`. Its controls are commands too: `view.textSize {size}` (11 to 18, null for 14), `view.zoom {zoom}` (0.8 to 1.6), `theme.setToken {token, color, theme?}` (replies with the token's contrast `checks`), `theme.setFruit {fruit, color, theme?}`, `theme.pinFamily {family, to}` (a fruit's name or `#rrggbb`; null unpins), `theme.reset {theme?}`, `theme.import {path}` and `theme.export {path, theme?}` (`.plumtheme.json`). `engine.auto {on}` is the Engines tab's checkbox and `run.cacheReport {open}` the Cache tab's button.
- `rename`: the rename window while it is open, `{kind: "node" | "frame", id, text}`, else null. `command.run node.title` or `frame.set` renames without it.
- `canvas.editing` is true while a widget edit is in progress (a drag, or a text field with focus) and not yet committed.

## Example session

```sh
python3 - <<'EOF'
import json, socket
s = socket.create_connection(("127.0.0.1", 7878)); f = s.makefile("rw")
def call(method, params=None):
    f.write(json.dumps({"id": 1, "method": method, "params": params or {}}) + "\n"); f.flush()
    return json.loads(f.readline())
print(call("auth", {"token": open("/path/to/control.token").read().strip()}))
print(call("command.run", {"command": "node.add", "params": {"kind": "text.upper", "pos": [0, 0]}}))
print(call("ui.set", {"fit": True}))
print(call("ui.screenshot", {"path": "/tmp/plumcraft.png"}))
EOF
```

`claude-debug/editor/control_probe.py` drives a whole session (add, wire, run, wait, inspect, screenshot, quit). `claude-debug/editor/m3_probe.py` drives the P08 flow on VectorCraft (build open → trace → recolor → SVG, run, rerun cached, watch the SVG and a document, `doc.open`, cancel a long trace).

## Known gaps

- There is no `ui.pointer` or `ui.key` yet: widgets are driven through `command.run` (`node.set`), not synthetic input; `ui.invoke` runs a menu item.
- No method closes the window; quit the editor with a signal.
- egui does not apply the canvas layer's transform to accessibility bounds, so widget rects inside nodes (in kittest or assistive tools) are in graph coordinates; the kittest tests map them with `canvas.to_global`.
- No MCP wrapper yet.
- The v1 projection of the research software's reply shapes (plan D21) isn't applied: PlumCraft's `ui.inspect` has `undo` and `redo` (labels) but no `save`, `viewer` or `params`, its `groups` has no checkpoints, and its `engine` is the protocol 2 `EngineState`.
