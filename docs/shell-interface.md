# The editor core ↔ shell interface, shell interface 1 (normative draft)

*2026-11-20. Status: **Draft 2, shell interface 1, built through stage N2** (`docs/research/24-native-shells.md` 17). Draft 1 (2026-11-19) was written before anything was built; the "As built" notes through the sections below say where N1 and N2 differ from it, and Draft 2 makes them the contract.*

**Changes from Draft 1 (N2, slices 1 to 4):**

1. **The selection and the canvas's actions are commands** (12.3): `selection.set`, `selection.clear`, `selection.all`, `net.select`, `edit.delete`, `canvas.focus`, `canvas.fit`, `canvas.palette`, `canvas.reveal`, `selection.align`, `selection.distribute`, `geometry.set`.
2. **The scene is built** (8, `scene.get`), with the shapes listed under section 8's "As built"; its types are the schema's (`crates/editor-core/schema/shell1.json`, 3.1 rule 4).
3. **Colours: a token by name alone, or `[r, g, b, a]` where there is none** (question 13, decided 13A, built in slice 4). `theme.get` resolves every token a scene names (14.1).
4. **`nid`s are unique within a document across every kind of item** (faces, wires, nets, frames, groups, marks: one counter), never reused; `plum_wire_paths` carries all 64 bits (9.1).
5. **`geometry.set`** (8.11): pins and rows as lists in row order, `rev` optional, a stale report kept and listed, `unknown`, `parts`, and (slice 4) `offset`. A new graph's fit is asked once every face is measured (8.9).
6. **Hit testing, the wire target and wire paths** (9): `plum_hit` with `status`, `item` and `x, y`; `plum_wire_target` naming its pin as (face, side, index); wire paths as four cubics fitted to egui-snarl's curve (8.8, question 7).
7. **`canvas.fit`'s rect is the core's** (8.9, `view.fit` in the scene).
8. **`ui.gesture` is how every shell's recognised gestures enter the core**, not only a script's (6.3, 8.13), with `plum_gesture_move` for a drag's moves (9.6). Draft 1 had a shell send commands itself; that put the gesture's meaning in the shell (rule 2).
9. **`geometry.locate` and `theme.get`** are new methods (6.3).
10. **The conformance suite is built** (16): cases as data, one runner, drivers for the C ABI, its control port, the egui editor and the core's own document.
11. **`PLUM_ABI_VERSION` stays 1** (5.2, "The ABI's version").

**Changes since Draft 2 (N3 slice 1, 2026-11-20; additive, so still Draft 2 and shell interface 1):**

12. **Moves are undo steps** (the maintainer's N2 exit decision 3; 8.13 "Undo", 12.6): `node.move`, `group.move`, `frame.move`, `frame.resize` and the new `edit.move` each make one undo step, named for what moved ("Move Node", "Move 3 Nodes", "Move Group", "Move Frame", "Resize Frame"); the moves of the same things that follow one join its step when they name the same `drag`, or name none and come within a second. A face drag through `ui.gesture` is one `edit.move` (it was a `node.move` or `group.move` a face). New: `selection.nudge {by, drag?}` (the arrow keys).
13. **A new wire dropped on empty canvas opens the palette for it** (question 8, decided 8A; 8.13, 12.7): `canvas.palette` takes `wire`; **the palette is the core's state** (open question 4, decided A), in the scene's `view.palette`, so `canvas.palette` is no longer refused by a core without a canvas; new: `palette.items` (a method), `palette.choose` (a canvas command) and `node.addWired` (a session command), which adds the node and connects the wire in one undo step.
14. **`ui.inspect` has `watched` from the core** (13.1): the watched value's node, port, summary, media, default viewer, its value or excerpt inline, and the references to read it by, on every shell (the egui shell adds its panel's fields over it).
15. **The conformance suite has two halves** (16.1): every case is `read` or `edit`; N3's gate is the read half. Cases can need an engine or `media.get`, and steps can wait on a condition (`until`).

*The rest of this paragraph is Draft 1's: Stage N0(c) of `docs/research/24-native-shells.md` (the plan; its twelve decisions were taken as recommended). Stage N0(c) of `docs/research/24-native-shells.md` (the plan; its twelve decisions were taken as recommended). Sibling of `docs/interface.md`, which is the contract between the editor and an engine; this file is the contract between the toolkit-free editor core (`plumcraft-editor-core`, not yet written) and a platform shell. Where this file says "today" it describes `main` at `34f6aea` and cites `file:line`; everything else is proposed.*

The words MUST, MUST NOT, SHOULD and MAY are used as in RFC 2119.

## 1. Scope

This interface is everything that crosses between the editor core and a shell:

- opening a core and its documents, and the platform's declaration of what it has (section 4);
- the three bindings: in-process Rust, the C ABI, the control channel (section 5);
- requests and replies, the methods kept and the methods added (section 6);
- the change feed a shell polls after a wake (section 7);
- the canvas scene, and the geometry a shell measures and reports back (section 8);
- the hot calls that keep dragging off the cold path (section 9);
- bytes: borrowed pictures and values (section 10);
- asks: what the core needs the platform to do, answered later (section 11);
- the menu and command model (section 12), viewers (section 13), themes and text (section 14);
- logging and diagnostics (section 15), conformance (section 16).

It does not cover engines (`docs/interface.md`), the workflow file, or how a shell draws. The control channel's framing and authentication stay as `docs/control-protocol.md` states them; this file adds methods to it and changes none of its replies.

**The floor.** Nothing the egui editor does today may be lost. Appendix A lists every piece of state and behaviour in `crates/ui` and the section that carries it.

## 2. Terms

| Term | Meaning |
|---|---|
| Core | `plumcraft-editor-core`: process-wide state (preferences, engine list, store, registries) and its documents. Owns no toolkit type |
| Document | One open workflow: a `Session`, its engine link, its selection, view and scene. One per window (decision 8) |
| Shell | The platform's code: drawing, text measurement, gesture recognition, native controls, windows, pickers. egui (`plumcraft-ui`), Swift (`native/apple`), Kotlin (`native/android`) |
| Client | Anything that sends requests: a shell, or a script on the control channel |
| Domain | A named part of a document's state that changes as a unit and has a generation (section 7.2) |
| Generation | A u64 per domain, strictly increasing within a document |
| Scene | The canvas as data: node faces, wires, frames, groups (section 8) |
| Face | One node (or collapsed group's proxy) as the scene describes it |
| Geometry | What a shell measured of a face: its size, pin anchors, row rects (section 8.11) |
| Ask | A request from the core to the shell, answered later (section 11) |
| Grant | An opaque string a shell keeps beside a path so it can reach the file again after a restart |
| Graph units | The canvas's coordinate space: a node's `pos`, at zoom 1 one graph unit is one point |
| Handle | A content address (`docs/interface.md` 4.1), or a derived picture reference (section 10.2) |

## 3. Roles, rules and versioning

### 3.1 Rules

These are plan section 3's rules, made normative.

1. **No toolkit in the core.** The core names no egui, AppKit, UIKit or Android type. Colours are `[r, g, b, a]` bytes, points and rects are `f32` pairs in graph units.
2. **No editing logic in a shell.** A shell MUST NOT decide what a gesture means, which wire may land on which port, what is selected, what a command's parameters are, whether a command is enabled, or how text about a run is worded. If a second shell would have to copy it, it belongs in the core.
3. **Everything is a command.** Every menu item, shortcut, context-menu item, toolbar button and gesture that changes a document ends in a command id with parameters (`AGENTS.md` rule 4). Section 12.3 lists what is not a command today and the id it gets.
4. **The shell interface is a contract.** A change to a message changes this file in the same commit, and the schema (`crates/editor-core/schema/shell1.json`, generated as `schema/protocol2.json` is: `cargo run -p plumcraft-editor-core --example export_shell_schema --features schema`, and a test fails while it is stale). Additions are capabilities (3.2).
5. **A shell is hostile input, and so is the core.** The core never panics on what a shell sends; the FFI edge catches unwinds (`catch_unwind` on every entry). A shell's parsers of core data MUST skip unknown fields, unknown enum values and unknown item kinds. Every new parser on either side joins a hostile-input sweep.
6. **The UI thread never waits for an engine.** No call a shell makes on its main thread may block on engine I/O, a file dialog, or another thread's lock held across I/O.

### 3.2 Versioning

- Every top-level message from the core carries `"shell": 1`, the shell interface major version. A reader that sees another major refuses with a clear error.
- **No minor versions.** An addition is a capability string. The core lists its capabilities in `core.hello`'s reply (`caps`); a shell lists its own in the open config (`shell.caps`). Either side uses a feature only when the other lists it.
- Capabilities of interface 1: `scene.deltas`, `geometry.rows`, `hot.wire_paths`, `hot.hit_test`, `hot.wire_target`, `asks.file`, `asks.prompt`, `asks.form`, `asks.text`, `asks.clipboard`, `asks.share`, `asks.open_external`, `asks.screenshot`, `bytes.borrow`, `log.ring`, `gesture.script`, `docs.multi`. *Draft 2:* `gesture.script` is `gesture` (every shell sends `ui.gesture`), with `hot.gesture_move`, `geometry.locate` and `theme.tokens` beside it. **No capability is listed by anything yet:** `core.hello` is not built, so a shell of interface 1 may use every method this file marks built. A shell lists the ask kinds it answers; the core never raises one it doesn't, and disables the commands that need it, with the reason (section 4.2).
- The core and the control channel speak the same version; a control client reads `core.hello` too.

### 3.3 Encoding and limits

- JSON, UTF-8, as `docs/interface.md` section 3: unions are internally tagged with `kind`; map keys and ids are strings; counters are unsigned 64-bit; readers ignore unknown fields; absent and `null` mean the default.
- A request is at most 1 MiB (the control channel's limit today, `docs/control-protocol.md`). A poll buffer has no fixed limit; a scene of 500 nodes is measured in N0.
- Text from engines (titles, errors, summaries, tooltips) reaches the shell unchanged. A shell MUST draw it as plain text: no markup, no links made from it, no format strings.

## 4. Opening a core and its documents

### 4.1 The core's config

`plum_open` (5.2) or `Core::open` (5.1) takes one JSON object:

```json
{"shell": 1,
 "app": {"id": "com.plumcraft.editor", "name": "PlumCraft", "version": "0.7.0", "build": "34f6aea"},
 "dirs": {"documents": "…/Documents", "caches": "…/Library/Caches/plumcraft", "store": "…/Library/Caches/commonkit/store",
          "scratch": "…/tmp/plumcraft", "prefs": "…/Library/Preferences/plumcraft", "logs": "…/Library/Logs/plumcraft"},
 "platform": {"os": "ios", "subprocess": false, "craft_apps": false, "python": false, "multi_window": true,
              "pointer": true, "keyboard": true, "hover": true, "pencil": true, "max_picture_px": 1280},
 "display": {"locale": "en-GB", "scale": 2.0, "text_size": 17.0, "dark": false, "reduce_motion": false, "frame_ms": 8.3},
 "shell": {"name": "apple", "version": "0.1.0", "caps": ["asks.file", "asks.prompt", "…"], "share_targets": [["photos", "Photos"], ["more", "More…"]]},
 "control": {"port": null, "token_file": null},
 "engines": {"default": "text", "mode": "inproc", "manifests": []}}
```

| Field | Meaning |
|---|---|
| `app` | The application's identity: the window title's name (today `EditorApp::app_title`, `crates/ui/src/lib.rs:499`), About, the prefs file's name, log lines. An application of the editor (the research software) gives its own |
| `dirs.documents` | Where documents live by default; a file ask starts here when no folder was remembered |
| `dirs.caches` | Copies for opening in apps (today `~/Library/Caches/plumcraft/open`), decoded rasters spilled to disk |
| `dirs.store` | The commonkit store root (`COMMONKIT_STORE_ROOT`). MUST be given on iOS, where the default is `/dev/shm` (plan 2.2) |
| `dirs.scratch` | Temporary files: saves waiting for `file.commit`, sends, screenshots. Swept by the core on open (`plumcraft_scratch`) |
| `dirs.prefs` | Preferences (`prefs.toml` today, `crates/ui/src/prefs.rs`), `labels.toml`, trusted workflows |
| `dirs.logs` | The bounded log file and diagnostics bundles (section 15) |
| `platform.*` | Section 4.2 |
| `display.locale` | BCP 47. Text the core makes is English in interface 1; the locale only formats numbers and dates |
| `display.scale` | Device pixels per point, for `px` (section 10.4) |
| `display.text_size` | The system's body size in points. The theme's text sizes are this times `view.textSize / 14` (section 14.3) |
| `display.frame_ms` | The display's frame period; the core coalesces scene and run changes to at most one per period (section 7.3) |
| `shell.caps` | Section 3.2 |
| `shell.share_targets` | `[id, label]` pairs a `share` ask may name (the spike's `SendTo::targets`). Empty: Send To is not offered |
| `control` | Section 5.3. Null `port`: no control channel |
| `engines` | What today's command-line flags choose (`--engine`, `--process`, `--engine-manifest`). The first document starts on it |

Unknown fields are ignored. A path outside `dirs` is never written by the core except where a file ask returned it.

**Since B151: which build this is.** `app.built: {commit, dirty, date}` is the native program's own build stamp, captured when it was built (the Apple apps: the `Stamp build` script phase, `native/apple/stamp-build.sh`, into the built Info.plist's `PlumBuildCommit`, `PlumBuildDirty` and `PlumBuildDate`), never by running git at run time: `commit` git's short hash, `dirty` whether tracked files had uncommitted changes (absent or null: unknown), `date` UTC as `YYYY-MM-DDTHH:MM:SSZ`. Anything not in that form reads `"unknown"`; a value that isn't an object is ignored. `app.about` replies `build: {line, shell, app, ui}`:

- `shell`: this library's stamp, made by `plumcraft-build-stamp`'s build script from the PlumCraft source it was built from.
- `app`: an application's own stamp, when its library set one (`plumcraft_ffi::app::set_build(plumcraft_build_stamp::stamp!())`, its build script calling `plumcraft_build_stamp::capture::emit`); else null.
- `ui`: `app.built`; else null.
- `line`: what About shows, one line a person can read out or copy: `Built 2026-11-24 21:51 UTC · shell 22f5884 · app 03d315b+changes` (`+changes`: built with uncommitted changes). Its date is the newest of the stamps' (normally the native program's, built last); `· ui <commit>` is added when the native program was built from another commit or state of changes than this library. Two builds made at different minutes, from different commits, or one with changes and one without, never read the same.

Every shell shows `line` as the core gives it, as selectable text under the version. The library's stamp is made again when git's `HEAD`, the branch, `packed-refs` or the index change, or anything under `crates/`, `Cargo.toml` or `Cargo.lock` does; only the stamp's crate and the libraries above it are built again. An edit git counts outside those paths (a document, a Swift source) leaves the library's flag as it was, which the native program's own stamp covers. With no git (a source tarball, a crates.io build) everything reads `"unknown"` and the build goes on.

**As built (N3 slice 4): `engines.also`.** The C ABI's config takes `engines.also: [engine id]` (at most 8): engines started beside `engines.default` in every document, each hosted its default way and checked against `platform` as the first is, behind the document's coordinator, so the document has one catalogue of both (what `engine.add` gives). An unknown id or a hosting the platform refuses is the config's error; the first engine, or one named twice, is started once. The Mac app opened every document on `text` with `array` beside it (both compiled in), so a graph could mix them and no conformance case lacked an engine; since the budget stage it starts only what a graph uses (`engines.auto`, below).

**As built (N3 slice 5): the iPad app's config.** `native/apple/PlumCraftPad` opens its one core with `platform: {os: "ios", subprocess: false, craft_apps: false, python: false, multi_window: true, pointer: true, keyboard: true, hover: true, pencil: true, max_picture_px: 2048}`, `display.frame_ms` 8.3, `engines: {default: "text", also: ["array"]}` (since the budget stage `{default: "text", also: [], auto: true}`), and every `dirs` root inside its container (`Documents`, `Library/Caches/plumcraft`, `Library/Caches/commonkit/store`, `tmp/plumcraft`, `Library/Application Support/plumcraft`, `Library/Logs/plumcraft`). Because `COMMONKIT_STORE_ROOT` wins over `store_root`, the app also sets it (and `COMMONKIT_MARK_DIR`, `PLUMCRAFT_CRAFT_ROOT`, `PLUMCRAFT_EDITOR_PREFS`) to its container before the core opens, whatever a launcher passed. The config is built by the kit's `ShellConfig` (`PlumCraftShell`), shared with the Mac app.

**As built (B149, 2026-11-23): one store for the core and every engine in the process, on every Apple shell.** An engine that stores through commonkit itself (an application's own, `4.5`) writes under `COMMONKIT_STORE_ROOT`, else commonkit's default folder, while the core reads `store_root`. The Mac app gave the core its store but set no variable, so started from the Finder (which passes none) such an engine's values were written where the core never looked, and their pictures were "missing". `ShellCore.open` now settles it before the application's engines are registered (`ShellCore.store`): a variable already set, by a script or by the person, is kept and the core uses it too (a leading `~` expanded); unset or empty, it is set to the app's store. The iPad app sets it itself first, so nothing changes there; the Windows app sets it to its store always (`ShellConfig.cs`). A scripted Mac launch (`--scratch-state`) may now leave the variable unset, as the Finder does: the store it then gets is checked to be under scratch like every other place it writes. `media.get` says why a value is `missing` in `reason`, as it already did for `failed`. Checked by the Mac person check's step "B149: launched as the Finder does with no store variable…" (`finder-store-check.py`), which an application gives its own graph.

**As built (the N3 budget stage, 2026-11-20): `engines.auto`, start only the engines a graph uses** (decision 8C at N3's exit). The config takes `engines.auto: bool` (default true; anything else is the config's error). With it:

- **A document opens on the engines its graph uses.** For `document.open {path}` the core reads the file before the document opens and works out its engines (`plumcraft_session::engine_cmds::graph_engines`): a node pinned to an engine uses it; a kind exactly one engine offers uses that one; a kind several offer uses one already used, else the first the workflow's `engines` names (spec 8.2), else `engines.default`; a kind no engine the core knows offers uses the engines the workflow names. Which engine offers a kind comes from the file, and, for the engines compiled in (text, array, an application's packs), from their own catalogues read once per process without starting them (`plumcraft_editor_core::engines::offered`, held as `Engines::offered`); a craft, a manifest's engine or the audio engine is known only when the file names it. Of those, the engines this platform can start start: `engines.default` first (hosted as `engines.mode` says) when the graph uses it, else the first used; the others beside it. A graph that uses none (untitled, empty), or a file that isn't a workflow read whole (a bundle, an application's own file), opens on `engines.default`. `engines.also` still starts its engines beside these in every document. The document's requests wait until all of them have connected, as before, so a graph of text nodes never waits for the array engine (slice 4's 270 ms).
- **The document picks engines automatically from then on** (`engine.auto` on, the egui editor's Pick Engines Automatically): a graph opened, or inserted, that uses an engine not connected starts it (`Session::missing_engines` now also names the engine a host knows offers a missing kind).
- **A node of another engine's kind added later starts that engine** (the defined path for N4's palette, which will list the kinds of engines not started from `Engines::offered`): `node.add` or `node.addWired` through `command.run` of a kind the catalogue lacks and an engine this platform can start offers asks for that engine (once: one that failed isn't tried on every add) and the request waits until it has connected, at most 30 s, then runs; requests for the document after it wait behind it, in order. A kind nothing here offers is refused at once, as before. The session's own `node.add` refuses such a kind naming the engine: "array.mandelbrot is the array engine's, which isn't connected; it is starting: add the node again once it has connected" with picking on, "…; the Engine menu starts it" with it off. **Since egui's parity work (after N4), the egui editor holds the add instead,** through the core's document (`Document::hold_adds`, `plumcraft_editor_core::document`): `node.add` and `node.addWired` (and so `palette.choose`) of such a kind ask for its engine and reply at once `{held: true, kind, engine, until}`; the add runs once the engine has connected (its kind in the catalogue), or the switch ended without it, or 30 s passed (`document::ENGINE_WAIT`), and is then one undo step "Add Node" where it was asked for. One add is held at a time (a second is refused while it waits); the requests after it are not held, since egui's own commands don't wait. The C ABI core leaves `hold_adds` off and holds the whole request, as above.
- **Engines stay once started:** a graph that stops using an engine (`graph.new`, nodes removed) keeps it connected until the document closes or `engine.clear`.

`engines.auto: false` keeps the old way: every document on `engines.default` and `engines.also` only, and picking off. Both Apple apps now open their core with no `engines.also` and picking on (their `--engines-also` and `--engines-pick off` remain, for comparisons); `ShellConfig.pickEngines` is the field. Tests: `crates/ffi/tests/engines_used.rs`, `crates/session/src/engine_cmds.rs` (`the_engines_a_graph_uses_…`, `a_missing_kind_an_engine_offers_…`), the kit's `aDocumentStartsOnlyTheEnginesItsGraphUses`. The conformance suite runs on each app opened on an empty graph: the text engine alone, until the picture case's first array node starts the array engine, so no case is skipped.

**As built (B71, 2026-11-21): engines from the engines folder, and installing them, in every shell.** Until B71 the C ABI's core read no engine manifests and could not install: both were wired by the egui application alone, so the native apps offered no engine an install had registered and Install Engine… was always disabled there ("this editor can't install engines"). Now the editor core does both for every shell. The engines folder is `dirs.engines` when the config names it, else `~/.config/plumcraft/engines`, the folder every PlumCraft editor shares (so an engine installed with the egui editor is offered by the native apps, and the reverse); `$PLUMCRAFT_ENGINE_FOLDER` wins over both, as `PLUMCRAFT_EDITOR_PREFS` wins over `dirs.prefs`, so scripts and tests point it at scratch. `engines.manifests: [file]` (at most 64) are read before the folder, as `--engine-manifest` is. The core thread reads them at start (what can't be read goes to the log) and `engine.manifests.reload` reads them again; a reload or an install in one document is offered by every document of the core. Whether `engine.install` is enabled is decided by the platform: `subprocess` and `python` both true (`plumcraft_editor_core::install::refusal`); else it is disabled with the reason, which `engine.list` gives as `installRefused`. The install is the core's (`install::install_engine`): `uv tool install --reinstall git+https://github.com/owner/repo`, then each command it installed's `register --folder <engines folder>`; `uv` is `engines.uv` when given (tests), else found as a manifest's `uv` is (`$PLUMCRAFT_UV`, PATH, the login shell's PATH, `~/bin`, `~/.local/bin`, `~/.cargo/bin`, `/opt/homebrew/bin`, `/usr/local/bin`), and a missing one fails at once saying where it looked. It runs on a thread of its own; each step it starts gets the search PATH, runs in a process group of its own and is stopped after 30 minutes. While it runs, `engine.list` and `ui.inspect`'s `engine` have `installing` (the URL) and `installStatus` (what it is doing, with uv's latest line), the `document` domain has `installing: {repo, status}`, and `lastError` (the status line, a native shell's banner) says it has begun; `engine.install.cancel` (Engine › Cancel Install) stops it and everything it started. When it ends the manifests are read again and `lastError` says how it went ("Installed and registered duckdb-engine from …. The Engine menu offers it now.", "The install of … was cancelled", "Installing … failed: …"), each also in the log (`install`).

**As built (N4 slice 4): preferences, appearance, About, and the control port beyond loopback.** `dirs.prefs` is the folder of the core's `prefs.json` (`PLUMCRAFT_EDITOR_PREFS` wins); `display.dark`, `display.contrast` (`"more"`), `display.reduce_motion`, `display.reduce_transparency` give the system's appearance at start (14.2); `app: {name, version, build}` is what `app.about` says (title, version, and parts: Build, Editor core, Shell interface, C ABI; since B151 `build`, 4.1). `control` takes `bind` (`"loopback"`, the default; `"lan"`, the device's address on its local network, the one it would send from, refused unless it is a private, link-local or unique-local address; or an address, which must be on this machine's local network, never `0.0.0.0`), `token` (a pre-shared 64-hex token, never printed) and `network_debug`. **A bind beyond loopback is refused unless `network_debug: true` and a pre-shared token (`token` or `token_file`) are given**, and a release build of an Apple shell can't set either: `ShellConfig`'s `controlBind` and `controlToken` exist only under `#if DEBUG`, and a release app given `--control-bind` says it refuses and listens on loopback. Beyond loopback the server closes a connection from a peer that isn't on the local network before reading it; every connection still authenticates first. `core.control` replies `{port, tokenFile, bind, loopback}`. The iPad's debug builds carry `NSLocalNetworkUsageDescription`; its release builds don't. **Since B83 (2026-11-21)** the control port honours a request's top-level `doc` as `plum_request` does: that document's answer, the first document's when it names none, and `code: "unknown_doc"` for one that isn't open (`docs/control-protocol.md`); before, every request was answered for the first document.

### 4.2 Platform capabilities

| Capability | When false |
|---|---|
| `subprocess` | Process, Python and HTTP-spawned engines are listed in the Engine menu and disabled with "this device can't start programs". `engine.attach` (a remote engine) stays. Install Engine… is disabled with "This device can't install engines: it can't start programs" (B71) |
| `craft_apps` | `doc.open` and Open in … are disabled with the reason; a craft document shows its rendition |
| `python` | Python engine modes are disabled with the reason; Install Engine… with "This device can't install engines: it has no Python" (B71) |
| `multi_window` | `view.popout` opens the pop-out as a sheet or pane of the same window; at most one document is open |
| `pointer` | No hover: section 8.10 lists what hover does today and what replaces it |
| `keyboard` | Shortcuts are still in the menu model; the shell may not show them |
| `hover` | As `pointer`, for a stylus or trackpad that hovers without a pointer |
| `pencil` | Reserved for the research software's mask painter (plan 5.5) |
| `max_picture_px` | Sent to engines as a hint (a run's `limits`), and caps `px` (section 10.4). Replaces the spike's 1280 px cap |
| `share` | The shell has no share sheet: `graph.share` (File › Share…, the toolbar's Share) and `value.share` (a pop-out's Share) are disabled with the core's reason "Sharing isn't available in this editor", in `menu.get`, `command.list` and the toolbar, and refused with it when run or invoked (`… is not available: …`); no `share` ask is raised. **Unlike the others it is true when unsaid** (built 2026-11-21) |

The mechanism is today's: a command's `enabled` returns `Err(reason)` (`crates/session/src/commands.rs:11`), shown as the disabled item's tooltip and in `command.list`'s `disabledReason`.

**As built (N1 slice 2, 2026-11-19).** `plumcraft_editor_core::engines::Capabilities` holds three of these so far: `subprocess`, `craft_apps`, `python`. A desktop's default has all three; a config read through the C ABI declares only what its `platform` object states, so one without `platform` declares none. `refuses(kind, mode)` gives the reason a hosting can't start, the most specific first: "‹Craft›: this device has no craft apps", "this device has no Python", "this device can't start programs". Without `subprocess` what is offered is text and array in-process, a running HTTP engine a manifest names, and `engine.attach`; an HTTP engine the editor would have to start is refused, like a process engine. `offers(caps)` lists every hosting with its reason. The C ABI's config reads `engines.default`, `engines.mode` and `platform`.

**As built (2026-11-21): `platform.share`.** Asked whether the egui editor should keep answering Share by showing the file in the file manager, the maintainer said "Disable". A shell says it has no share sheet with `platform.share: false` in the C ABI's config, or, in process, `Document::share = false` (`plumcraft_editor_core::document`); the egui editor does the latter. Since every shell built before it shares, `share` is on when the config doesn't mention it (any value but a boolean is the config's error), so the Apple apps keep their share sheets and the Windows app keeps answering the `share` ask with its own error ("Sharing isn't available yet in PlumCraft for Windows."), both unchanged. The rule is the core's (`canvas_cmds::shell_refuses`, its reason `canvas_cmds::SHARE_UNAVAILABLE`), read through the menu facts (`menu::Facts::no_share`), so the menu model, the toolbar, `command.list`, `ui.invoke` and `command.run` all say the same thing, and a shell's own button (egui's pop-out bar) asks the core for it rather than greying itself. Tests: `crates/ffi/tests/share.rs`, `canvas_cmds::tests::a_shell_without_a_share_sheet_has_share_refused_with_the_cores_reason`, `kittest_popout_bar::share_is_disabled_because_the_core_says_this_editor_cant_share`.

**As built (2026-11-21, decision 12 after the person check, B91's core half): a hosting whose program isn't there.** A built-in engine hosted as a process or over HTTP (`Mode::Rust`, `Mode::HttpRust`: Text, Audio, Array, the craft adapter) starts its program, `plumcraft-<name>-engine`, found where `engine_binary` looks: beside the app's own program (an app bundle's `Contents/MacOS`, the Windows app's folder), else, for a cargo build or test, the build's profile folder. When it is in neither, the hosting is listed and disabled with "This app doesn't include the Text engine program (plumcraft-text-engine)" (`engines::missing_program`, through `Capabilities::refuses`, so the Engine menu, `engine.list` and the command line say the same), instead of being offered and never connecting. Compiled-in and Python hostings, an engine a manifest names and one already running are not judged so. **The Windows app must ship** `plumcraft-text-engine.exe` and `plumcraft-array-engine.exe` beside `Plummer.exe` (and `plumcraft-audio-engine.exe` and `plumcraft-craft-engine.exe` to offer those as processes); what it doesn't ship is now shown disabled with that reason.

**As built (2026-11-22, the maintainer: "Compile array,audio,text in"): the audio engine in process.** A core opened through the C ABI has the audio engine compiled in beside text and array: `engine.list` offers `audio` in process (its default hosting), its kinds are known before it starts (so adding one starts it, 4.1), and on a platform that starts no programs (the iPad) it is offered in process while its process and HTTP hostings are listed disabled with the reason. The egui editor, which is published and so can't link the engine's SoundCraft git crates, still starts it as a process (`engines::set_compiled_audio` is how an application compiles it in).

### 4.3 Core and documents

- **The core** is process-wide: preferences and the theme, the engine list and manifests, the store, the command, form, hook, viewer and value-editor registries, the log ring, the control channel.
- **A document** is one window (decision 8): its own `Session`, engine link, selection, view, scene, watched values, pop-outs, asks and generations. Requests that act on a document name it (`doc`, section 6.1).
- **Opening:** `document.open {path?, grant?, demo?}` → `{doc}`. Without `path`, an untitled document (today `graph.new`'s state, named `Untitled`, `crates/ui/src/lib.rs:540`). With `demo: true`, the engine's demo graph (today's no-argument launch).
- **Closing:** `document.close {doc, discard?}`. Over unsaved changes without `discard` the core raises the `prompt` ask `unsavedChanges` with `then: "close"` (today `Prompt::Discard { then: Quit }`, `crates/ui/src/lib.rs:1089`) and replies `{closing: false, ask}`; once nothing is unsaved it replies `{closing: true}` and the document's state goes. The shell closes the window after `closing: true`, or after the feed's `document` domain says `closing`.
- **Quitting:** `app.quit` (a command) closes every document in turn, each with its own prompt.

**As built (N5 slice 3): a window per document, and the close prompt's words.** The Windows app, which makes a window for every document, has the core decide what its document windows do, so no shell-side action is left (golden rule 4, the maintainer: "No exceptions to the rule"):

- **`platform.document_windows: true`** in the C ABI's config (default false; anything but a boolean is the config's error) makes every document a window of the shell's. Then `ui.invoke graph.new` opens a new untitled document (nothing about this one's unsaved changes is asked: nothing is discarded); `ui.invoke graph.open` asks `file.open {purpose: "workflow"}` and opens the file in a new document, or in this one while it is untouched (no file, nothing unsaved, no nodes: a platform document controller's rule); and `ui.invoke app.quit` closes every document in turn, the one it was chosen in first, each asking first over unsaved changes (`unsavedChanges` with `then: "quit"`), a cancelled prompt (or a cancelled save) stopping it with the rest open. Each new document is told to the shell in the feed as `changed {doc, domain: "window", open: true, from, by, path?}`: make a window for `doc`, which New or Open in document `from` made, for the person or a script. A shell with no window left ends, as a platform's does. `plumcraft_editor_core::invoke::Host` gained `document_windows`, `open_document` and `quit_all` (defaults: off, nothing, false), so the egui editor and every other host are unchanged; the document's `opens` and `quit_asked` are what the C ABI core takes from it each turn.
- **`document.close {doc, discard?, by?}`** (`by`: `"person"`, or `"script"`, the default) now asks in words that say the window closes: `unsavedChanges` with `then: "close"` and the text "The graph has unsaved changes. Discard them and close the window?" (it said "…and quit?"), for the person when the shell's close button says `by: "person"`, so the shell shows it. This is for every C ABI shell: the iPad's close (which sends no `by`) now reads "close the window"; the Mac closes through NSDocument with `discard: true` and never sees it.
- **Without `document_windows`** (the iPad app, whose document browser makes its scenes, and the default) New and Open replace the document's graph and Quit closes the document it was chosen in, as before.
- **The Mac declares it (decision 20, "Mac adopts it", 2026-11-21).** File › New and File › Open… in a window run `ui.invoke {command: "graph.new" | "graph.open", by: "person"}`; the core makes the document (or opens the file into the untouched one, which the Mac then follows from the `document` domain's `path`: the window's file, title and proxy icon move to it) and the feed's `window` change brings an NSDocument and its window for it (`native/apple/PlumCraftMac/App/DocumentWindows.swift`), noted in Open Recent unless the launch is scripted. The Open panel for `file.open {purpose: "workflow"}` is a window of its own, as the Mac's Open is, not a sheet. What stays AppKit's, because the core's ownership and AppKit's document architecture can't both hold there: New and Open with no window open (no document to run them in: NSDocumentController, then `document.open`), files from the Finder, a drop or the launch (NSDocumentController, then `document.open {path}`); Open Recent is the core's since B166 (12.24), closing a window (NSDocument's Save, Don't Save, Cancel sheet, then `document.close {discard: true}`), and Quit (AppKit's review of unsaved documents, which the system's own quit at log out also goes through, answered from `applicationShouldTerminate`). Those are a decision for the maintainer (note 24, "The Mac's document windows").
- Tests: `crates/ffi/tests/documents.rs` (`with_a_window_per_document_new_open_close_and_quit_are_the_cores`, `without_a_window_per_document_new_and_quit_stay_in_their_document`), the Windows kit's `Editing.WithAWindowPerDocumentNewIsADocumentAndClosingSaysClose`. Not in the conformance suite: `document.close` is a C ABI method, not the control channel's, and `document_windows` is a shell's config.
- **Process-wide state today.** These are `static` and process-global, which two documents must share safely or stop sharing (the N1 audit, plan 4.3): the command registry and app ids (`crates/session/src/commands.rs:256-260`), hooks (`:138`), forms (`:230`), the viewer registry (`crates/ui/src/viewers/mod.rs:113`), value editors (`crates/ui/src/value_editors.rs`), openers (`plumcraft_session::file_cmds::openers`), the hash algorithm (`plumcraft_proto::handle::Algo`), the spike's Send To targets. Hooks are `fn()` with no document argument, so an application's overlays cannot tell two documents apart (open question 3).

### 4.4 Life cycle and memory

| Method | Params | Core does |
|---|---|---|
| `lifecycle.background` | `{}` | Writes the journal and each dirty document's recovery copy; on a platform without background running (iOS), asks for background time and cancels runs still in flight when it ends (`run.cancel` per document). Replies when that is done |
| `lifecycle.foreground` | `{}` | Reconnects engines that went away while in the background; reruns a watched value whose bytes were reaped |
| `lifecycle.memory` | `{level: "low" \| "critical"}` | `low`: drops decoded rasters and the `Fulls` cache (`crates/ui/src/watched.rs:42`) not in use. `critical`: also drops thumbnails and every borrowed-but-released byte buffer, and lowers `px` to `DEFAULT_PX` (1024, `crates/session/src/viewer_cmds.rs:29`) |

Autosave on iOS and Android (decision 7) is a document option, `document.open {autosave: true}`: the core saves to the document's file once a change has settled; desktop keeps explicit Save.

**As built (N4 slice 4).** The maintainer: "Build autosave in the core and use it on the iPad; the Mac keeps explicit Save." The four methods are the C ABI core's (`plum_request`; the control channel doesn't serve them):

| Method | Params | Reply | What the core does |
|---|---|---|---|
| `lifecycle.background` | `{}` | `{saved: [doc], failed: [{doc, error}]}` | Writes every autosaved document now (`plumcraft_editor_core::autosave::flush`), waiting at most 10 s for a save in flight; replies when done, so the shell holds its background time until then. Runs go on: a platform that suspends cancels them itself (`run.cancel`) when its time ends. No journal or recovery copy is written (none exists yet) |
| `lifecycle.foreground` | `{}` | `{}` | Nothing yet: the in-process engines don't go away in the background |
| `lifecycle.memory` | `{level: "low" \| "critical"}` | `{level, before, after}` (media stats) | Drops the media service's decoded values not in use (both levels; the core holds no other thumbnails) |
| `lifecycle.appearance` | `{dark?, contrast?: "more" \| "normal", reduceMotion?, reduceTransparency?}` | `{changed, chosen, shown, system, gen}` | The system's appearance, over what it was (section 14.2) |

**Autosave** (`crates/editor-core/src/autosave.rs`), on for a document opened with `autosave: true` or by `document.autosave {on: true}` (6.3):

- **When:** once a change has settled (1 s with no other), or at most 10 s after the first unsaved change while changes keep coming; never while a run is in flight (the change waits for the run to end); never for a graph with no file (state `untitled`). Every change that dirties the graph counts (the session's `changes()` count: edits, moves, undo and redo, and the changes that aren't undo steps, Show on Face's `node.face`, `node.fields`, `node.view`).
- **How:** serialised on the core's thread, written on a thread of its own, atomically (a temporary file beside it, synced, renamed), one save at a time per document. A save of change *n* cleans the document only if nothing changed while it was written.
- **`dirty`** means "not yet saved by autosave".
- **Failure** (a read-only folder, a provider offline): state `failed` with the reason; the document stays dirty; tried again after 5 s, doubling to a minute, or as soon as the next change settles. Nothing modal: the shell shows the state.
- **Flush:** `lifecycle.background`, `document.close` (an autosaved document is saved as it closes; only a failed save leaves it open and raises the unsaved-changes prompt) and closing the core.
- **The feed:** the `document` domain carries `autosave: {on, state, error?, saves, failures}`; `state` is `off`, `clean`, `pending`, `untitled`, `saving`, `saved` or `failed`.
- **Untitled documents on the iPad** live in the app's own folder (Files › Plummer, B147): the browser's + makes `Untitled.plum` there (`Untitled 2.plum` and on when that is taken; the extension is the core's, 12.23); every graph the iPad shows has a file.

### 4.5 An application's own engines

An application with an engine of its own (an engine pack's, compiled in) runs it in the native shells' process as the egui editor's `AppConfig.packs` and `AppConfig.openers` do there, with no change to the shells but where they find the library:

- **Its library.** The application builds a static library of its own around `plumcraft-ffi` (a crate that depends on it and on the engine): every `plum_*` entry is there, unchanged, plus one entry of the application's own that registers its engines and file openers through the Rust API, before the first `plum_open`:

  ```rust
  // plumcraft_ffi::app
  pub use plumcraft_editor_core::engines::PackEngine;   // id, label, engine: fn() -> Result<Engine, String>, demo, worker
  pub use plumcraft_session::file_cmds::FileOpener;      // extensions, label, read: fn(&Path) -> Result<String, String>
  #[derive(Debug, Clone, Default)]
  pub struct App { pub packs: Vec<PackEngine>, pub openers: Vec<FileOpener> }
  pub fn register(app: App) -> Result<(), String>;
  pub fn registered() -> Vec<String>;                    // the registered engines' ids, in order
  ```

  `register` is once per process and refused, with why, after a core has opened (a core reads the engines as it opens), for a pack with `worker: true` (a native shell's process starts no copy of itself: an application's engine is hosted in process), for an id that is empty, not a plain name, or already an engine's (`text`, `array`, `audio`, a craft's, another of the application's), and for an extension already taken (a workflow's `json` or `zip`, or another opener's). Nothing is registered when anything is refused. The C ABI is unchanged (no entry, no version change): the registration is Rust's, and the application's entry is its own.
- **Registered,** an engine is offered in process (`engine.list`'s `choices`, the Engine menu), on every platform whatever its `platform` says (it starts no program); `engines.default` may name it; its node kinds are known before it starts, so a document opens on it and the palette lists its kinds (`engines.auto`); and its openers open their files as workflows (`graph.open`, File › Open).
- **The Apple kit.** `native/apple/PlumCraftFFI/Package.swift` links the xcframework `PLUMCRAFT_FFI_XCFRAMEWORK` names (a path relative to the package; SwiftPM takes no absolute one), else `PlumCraftFFI.xcframework`. The module map links the library as `plumcraft_ffi`, so the application's is named so in its xcframework. Before `ShellCore.open` opens the core it calls `register()` on the app's class named `PlumAppLibrary` (`@objc(PlumAppLibrary)`, conforming to `PlumCraftShell.AppLibrary`) when the app has one; that calls the application's entry. PlumCraft's own apps have none. The iPad app's name in `app.name` (About) is its bundle's display name.
- **Not yet:** Android's and Windows' kits take only PlumCraft's own library. An application's pack types and value editors are its engine's catalogue and the core's generic views; an application's own Swift editors are its own target's.
- **Tests:** `crates/ffi/tests/app_engines.rs` registers the public test engine (`crates/ports`) and a file opener, checks each refusal, opens a core on a platform that starts no programs with the test engine as `engines.default`, finds it offered in process, opens a file through the opener, and runs its graph to the engine's result.

## 5. Bindings

### 5.1 In-process Rust

The egui shell and the tests link the core. Sketch, provisional until N1:

```rust
pub struct Core { /* … */ }
impl Core {
    pub fn open(config: &Value, waker: Arc<dyn Fn() + Send + Sync>) -> Result<Core, String>;
    pub fn request(&self, json: &[u8]) -> u64;                 // asynchronous: the reply comes in poll
    pub fn call(&self, doc: DocId, method: &str, params: &Value) -> Result<Value, String>; // synchronous, for tests and egui today
    pub fn poll(&self) -> Feed;                                 // section 7
    pub fn snapshot(&self, doc: DocId) -> Arc<Snapshot>;        // section 9: lock-free
    pub fn blob(&self, handle: &str) -> Borrow;                 // section 10
}
```

`call` runs on the caller's thread and holds the document's lock for the command's duration, as `EditorApp::try_run` does today (`crates/ui/src/lib.rs:738`). It MUST NOT be used for methods that wait on asks or engines; those reply through `poll`.

### 5.2 The C ABI (`crates/ffi`)

Plan section 7's sketch, extended with a document argument where the plan's had none. **Another agent is validating it in `crates/ffi`; every signature here is provisional** and that crate's header wins where they differ.

```c
typedef struct plum_core plum_core;
typedef struct { uint8_t *ptr; size_t len; } plum_buf;                 // owned by the core until plum_free
typedef struct { const uint8_t *ptr; size_t len; uint32_t w, h, stride; uint32_t format; uint32_t status; uint64_t token; } plum_bytes;
typedef struct { uint32_t kind; uint32_t part; uint64_t node; uint64_t pin; uint64_t wire; int32_t index; uint32_t fits; } plum_hit;

plum_core *plum_open(const uint8_t *config_json, size_t len, void (*wake)(void *), void *ctx);
void       plum_close(plum_core *);

uint64_t   plum_request(plum_core *, const uint8_t *json, size_t len);     // cold path, asynchronous
plum_buf   plum_poll(plum_core *);                                          // section 7
void       plum_free(plum_buf);

size_t     plum_wire_paths(plum_core *, uint64_t doc, const uint64_t *nodes, size_t n, float dx, float dy, float *out, size_t cap);
plum_hit   plum_hit_test(plum_core *, uint64_t doc, float x, float y, float slop);
plum_hit   plum_wire_target(plum_core *, uint64_t doc, uint64_t from_pin, float x, float y, float slop);

plum_bytes plum_blob(plum_core *, const char *handle);
void       plum_release(plum_core *, uint64_t token);
```

**As built (N2 slice 3), the hot calls** (`crates/ffi/include/plumcraft.h` is the reference):

```c
typedef struct { int32_t status; uint32_t kind, part; int32_t index; uint64_t item, node, wire; uint32_t pin, fits; float x, y; } plum_hit;
typedef struct { uint64_t face, rev; float w, h, header; const float *inputs; size_t n_inputs; const float *outputs; size_t n_outputs;
                 const float *rows; size_t n_rows; } plum_face;
plum_hit plum_hit_test(plum_core, uint64_t doc, float x, float y, float slop);
plum_hit plum_wire_target(plum_core, uint64_t doc, uint64_t face, uint32_t side, uint32_t pin, float x, float y, float slop);
size_t   plum_wire_paths(plum_core, uint64_t doc, const uint64_t *faces, size_t n, float dx, float dy, float *out, size_t cap);
int32_t  plum_geometry_set(plum_core, uint64_t doc, const plum_face *faces, size_t n);
```

- A core is a number (`plum_core`), as N0 built it; `doc` 0 is the first document open.
- **`plum_hit` gains `status`** (`PLUM_OK`, `PLUM_ERR_CORE`, `PLUM_ERR_MISSING` for no such document or no scene published for it yet, `PLUM_ERR_ARG`, `PLUM_ERR_PANIC`), **`item`** (the nid of whatever was hit, by `kind`) and **`x, y`** (a pin's point, where a dragged wire snaps). `pin` is the pin's index on its side, `part` 1 input or 2 output (9.2).
- **`plum_wire_target` names the pin as (face nid, side, index)**, side 1 an input and 2 an output, not one `from_pin` number: a pin has no nid of its own, and a wire's ends are already face and index (`fromFace`, `fromPin`).
- **`plum_geometry_set`** is `geometry.set` typed: copied before it returns, applied on the core's thread, faces by nid; no `parts` (the JSON form has them).
- The JSON forms are `geometry.hit`, `geometry.wireTarget`, `geometry.wirePaths` (methods) and `geometry.set` (a command), on the control port too (`docs/control-protocol.md`).
- JNI has no hot calls yet (N5).

**As built (N2 slice 4):**

```c
typedef struct { int32_t status; uint32_t kind; uint64_t serial; float dx, dy; float rect[4]; plum_hit target; } plum_drag;
plum_drag plum_gesture_move(plum_core, uint64_t doc, float x, float y, float slop);
```

- **`plum_gesture_move`** is `ui.gesture`'s drag `move`, typed (9.6): the drag began, and ends or is cancelled, through `ui.gesture` (JSON); its moves never cross as JSON.
- **`plum_drag_wire(core, doc, x, y, slop, float *out, size_t cap) → size_t`** (B158) is the move's `update.wire`, typed: for a bend point's or a route's drag, two records in `plum_wire_paths`' form, both by the wire's nid: its path as it would be, then its bend points (one point each). The floats needed are returned, and nothing is written when that is more than `cap`; 0 for any other drag, no drag, or a point that isn't a number.
- **`plum_wire_paths` carries the whole nid**: per wire its low and high 32 bits and its point count (three floats bit-cast), then the points. Slice 3 sent the low 32 bits only.
- **The snapshot reaches the hot calls as it is made**, before any reply that follows the publish goes out (each document hands the C ABI a cell it swaps). Slice 3 shared it at the end of the core thread's turn, so a hot call right after a reply could read the one before. Hot calls still never wait.
- **The ABI's version stays 1.** N2 added exports (`plum_hit_test`, `plum_wire_target`, `plum_wire_paths`, `plum_geometry_set`, `plum_gesture_move`) and changed one layout (`plum_wire_paths`' nid, slice 4) before any shell called it: nothing outside this repository links the library, and the only shell code that does (the N0 spikes under `native/`) calls none of the calls N2 added or changed. The version rises from the first change after a shell is released (N3) that would break it; additions never raise it (3.2).

- `wake` is called from any thread, with no payload, at most once per change batch; the shell hops to its main thread, coalesces, and calls `plum_poll`. No data crosses on a foreign thread (the Swift 6 isolation trap, plan 2.3).
- Every entry catches unwinds and returns an empty value (`len 0`, `kind 0`, `status` set) on a panic, logging it.
- JNI: JSON goes as `byte[]`, never `String` (modified UTF-8, plan 2.3).
- `plum_request` returns the request's number; a request's own `id` (any JSON) is echoed in its reply as on the control channel.

### 5.3 The control channel

The control channel moves into the core (plan section 9), so every shell has it. Framing, the token, the 30 s reply limit and every existing reply stay byte for byte (`docs/control-protocol.md`). It is a client like any shell, with these differences:

| Method family | From a shell | From the control channel |
|---|---|---|
| `command.*`, `graph.get`, `ui.inspect`, `ui.invoke`, `ui.prompt`, `ui.set`, `ui.waitRun`, `ui.audio` | Yes | Yes, unchanged |
| `ui.screenshot` | No (the shell is the one asked) | Yes: the core raises the `screenshot` ask to the shell (11.2) |
| `ui.gesture` | Yes (since N2 slice 4: how a shell's recognised gestures enter the core) | Yes (6.3) |
| `geometry.locate`, `theme.get` | Yes | Yes (N2 slice 4) |
| `ask.answer` | Yes | Yes: a script may answer any pending ask; the first answer wins |
| `geometry.set`, `ui.viewport`, `ui.report` | Yes | `geometry.set`: yes since N2 slice 3, a command like any other (the egui shell reports its own measurements every frame, so a script's lasts until the next frame there); `ui.viewport`, `ui.report`: no, a script cannot claim to have a view |
| `geometry.hit`, `geometry.wireTarget`, `geometry.wirePaths` | Yes (typed as hot calls) | Yes (N2 slice 3) |
| `document.*`, `lifecycle.*`, `state.get`, `scene.get`, `menu.get`, `log.tail` | Yes | Yes (read-only ones; `lifecycle.*` refused) |
| Hot calls (section 9) | Yes (typed) | No |

A control request with no `doc` acts on the focused document (the one the shell last named in `document.focus`), which is today's behaviour with one window. `document.list` names them all.

### 5.4 Threads

- The core runs its documents on a thread of its own; engine calls go to workers (today `pump_runs` calls `conn.call("run")` on the UI thread, `crates/ui/src/lib.rs:1407`, which N1 moves off it).
- `plum_request` returns at once; the reply arrives with a later poll.
- Snapshots (scene, geometry, menu model) are immutable behind an atomically swapped pointer; hot calls read the current one and never wait (section 9.4).

## 6. Requests and replies

### 6.1 Framing

A request: `{"id": <any JSON>, "method": "<name>", "params": {…}, "doc": <u64>?}`. A reply, delivered in the feed (7.1) or on the control channel:

```json
{"id": 1, "ok": true, "result": {…}}
{"id": 2, "ok": false, "error": "unknown method \"foo\"", "code": "unknown_method"}
```

`code` is new and additive: `bad_json`, `bad_params`, `unknown_method`, `unknown_doc`, `disabled`, `busy`, `timeout`, `not_allowed`, `internal`. `error` stays the human sentence it is today. Every request gets exactly one reply. **As built (seen by the Swift kit, N3 slice 2):** the C ABI sends `code` only where it refuses a request's `doc` (`unknown_doc`, `bad_params`); every other refusal, an unknown method included, has `error` alone. A shell treats `code` as optional.

### 6.2 Methods kept verbatim

| Method | Change |
|---|---|
| `command.run` (alias `engine.execute`) | None. Runs through the core's `try_run` (today `crates/ui/src/lib.rs:738`), including its follow-ups: `run.watch` reruns, `graph.new`/`graph.open` fit and clear selection, focus and watched |
| `command.list` (alias `engine.commands`) | None. Rows from `list_entry` (`crates/session/src/commands.rs:319`) |
| `graph.get` | None |
| `ui.inspect` | None to existing fields; built from the core's state plus what the shell reported (`ui.report`, `ui.viewport`). Gains `doc`, `asks` (pending asks), `docs` already exists for craft documents and is not renamed |
| `ui.invoke` | Same reply shape. Where a native dialog blocked the window until answered (`graph.open`, `graph.saveAs`, …), the core now raises an ask and replies at once with `invoked` and the ask in `asks` (open question 1) |
| `ui.prompt` | None. Answers the pending `prompt` ask (11.2) |
| `ui.set` | None. `zoom`, `pan` and `fit` become a `view` change the shell animates to (8.9); `palette` and `focus` act on core state |
| `ui.waitRun` | None |
| `ui.screenshot` | Same reply. The core raises `screenshot` (11.2); the egui shell keeps its offscreen fallback (`crates/ui/src/offscreen.rs`), a native shell renders its own view hierarchy offscreen |
| `ui.audio` | None. Playback moves to the core (section 13.4) |

### 6.3 New methods

**N4 slice 4:** `document.open {path?, demo?, autosave?}` (autosave: 4.4; the reply says `autosave`); `document.autosave {on?, path?, scratch?}` → `{on, state, error?, saves, failures, path, dirty, disk?: {exists, matches, nodes, frames}}`: autosave on or off for this document, a file to save to without writing it yet (`path`, or `scratch`: a file of that name in the core's scratch folder), and the state with what is on disk (`matches`: the file holds exactly the graph as it is now); served by the dispatcher, so on the control channel and every shell. `settings.get {fromDisk?}` → section 14.4. `lifecycle.*`: 4.4.

Each lists params, result and errors beyond `bad_params` (unknown fields are an error, as `ui.set`'s are today).

**`core.hello`** `{}` → `{shell: 1, caps: [..], app, core: {version, build}, hash: {algo, source}}`. The hash is the process's one algorithm (`docs/interface.md` 4.3), which the status bar shows today.

**`document.open`** `{path?, grant?, demo?, autosave?}` → `{doc, title}`. Errors: the open's own (`graph.open`'s). **`document.close`** `{doc, discard?}` → `{closing, ask?}`. **`document.focus`** `{doc}` → `{}`: the shell's frontmost document, for the control channel and the menu bar. **`document.list`** `{}` → `{docs: [{doc, title, path, dirty, focused}]}`.

**`state.get`** `{domains: ["document", "run", …]}` → `{<domain>: {gen, …}}`: the current state of each domain asked for (7.2). Unknown domains are skipped, not an error.

**`scene.get`** `{since?: gen}` → section 8.12. Built in N2 slice 2 (`plumcraft_editor_core::scene`), served by the one dispatcher on the egui editor's control port, the C ABI and the C ABI's own control port. `since` is a whole number (a scene `gen`); anything else, or another field, is refused with the reason. The first `scene.get` also starts the feed's scene changes for that document (7.1). **`menu.get`** `{context?: {kind, …}}` → section 12. **`theme.get`** `{}` → section 14.

**`geometry.set`** `{faces: [{node, rev, size, pins, rows?, header?}]}` → `{applied, stale}`. What the shell measured (8.11). `applied` counts faces taken; `stale` lists nodes whose `rev` is older than the core's (ignored; the shell measures again). Not journaled, no undo step. **As built (N2 slice 3):** a canvas command (`command.list`, `command.run`); `rev` optional; `pins` is `{in?: [y | null], out?: [y | null]}` in row order; also `parts`; the reply adds `unknown` (faces the graph doesn't have, skipped); **a stale face is kept all the same**, since an older measurement beats the default and a face measured during a stream of run changes would otherwise never land, and listed so the shell measures again. **`geometry.hit`**, **`geometry.wireTarget`**, **`geometry.wirePaths`**: section 9's hot calls as JSON (`docs/control-protocol.md`).

**`ui.viewport`** `{zoom, pan: [x, y], rect: [x0, y0, x1, y1], settled}` → `{}`. The canvas transform (`screen = graph · zoom + pan`, as `ui.set` defines it) and its rect in window points, reported when a gesture ends and at most four times a second during one (plan 5.3). `settled: true` at a gesture's end. `ui.inspect`'s `canvas` reads it.

**`ui.report`** `{palette?, rename?, editing?, popups?, windows?}` → `{}`. Transient state only the shell holds, for `ui.inspect` (open question 4): `editing` is `canvas.editing` (a field has focus or a drag is in progress, today `EditBuffers::is_editing`, `crates/ui/src/widgets.rs:42`); `popups` is `anyPopup`; `windows` the rects of the shell's windows.

**`ui.gesture`** `{kind: "tap" | "doubleTap" | "context" | "longPress" | "drag" | "marquee", phase?: "begin" | "move" | "end" | "cancel", at: [x, y], to?: [x, y], mods?: ["shift" | "primary" | "alt" | "ctrl"], slop?, zoom?}` → `{commands: [{command, params, ok, result | error}], hit?, drag?, update?, context?, menu?, cancelled?}`. *As built (N2 slice 4), from every shell and the control channel:* a gesture the shell recognised, in graph units; the core publishes the scene, hit-tests it with `slop` (graph units, default 12: the shell's target size over its zoom) and runs what it means (8.13). `context` is a secondary click or a long press (`longPress` is the same); `drag` has a `phase`; `marquee` is a rect in one go (`at` to `to`), what a Select mode or a Pencil drag sends. Pan and zoom (`pinch`, `pan`) are not gestures here: they never cross (plan 5.3). Draft 1's `space: "screen"` and `scale` are not built: a shell converts. An unknown kind, phase, modifier or field, a point that isn't two finite numbers within ±1e7, and a `move` or `end` with no drag under way are errors. With no geometry reported the core hits its own estimates (220 × 160 faces), not an error.

**`geometry.locate`** `{node, part?, pin?} | {wire} | {frame, part?}` → `{at, rect?, nid?, measured?}` (N2 slice 4): where the core has a face (its centre, header or body), a pin, a point on a wire's path, a frame's body, tab or handle, at the geometry it has; `measured` says whether the shell measured the face. Scripts and the conformance suite aim gestures with it, whatever sizes the shell measured.

**`theme.get`** `{}` → `{shell, name, tokens: {name: [r, g, b, a]}}` (N2 slice 4): every token a scene may name, in the theme the shell's scenes are built in. Draft 1's fuller reply (14.1: fruits, families, radius, spectrogram, text sizes) is not built.

**`ask.answer`** `{ask, answer?, cancel?}` → `{}`. Errors: `bad_params` for an answer of the wrong shape (the ask stays open), `unknown_ask` when it was answered, cancelled or withdrawn.

**`file.commit`** `{ask, ok, error?}` → `{}`. After a `file.save` ask whose answer was a scratch path (11.3), the core writes the file, sends `file.committing {ask, path}` in the feed, and the shell streams it to its destination and calls `file.commit`. Until then the save shows as in progress.

**`log.tail`** `{since?: seq, level?: "error" | "warn" | "info" | "debug" | "trace", target?, limit?: 1…5000}` → `{lines: [{seq, t, level, target, text}], next, dropped}`. Section 15.

**`lifecycle.*`** Section 4.4.

**`media.peaks`** `{handle}` → `{channels, bins, levels: [[min, max] …]}` or `{pending: true}`: a `peaks` rendition decoded (today `Thumbs::peaks`, `crates/ui/src/run_view.rs:205`). **`media.describe`** `{handle}` → `{dtype, shape, media, size}` for a borrowed value's bytes.

## 7. The change feed

### 7.1 What `poll` returns

```json
{"shell": 1, "seq": 9381,
 "replies": [{"id": 4, "ok": true, "result": {…}}],
 "changed": [{"doc": 1, "domain": "scene", "gen": 412}, {"doc": 1, "domain": "run", "gen": 77}],
 "asks": [{"ask": 18, "doc": 1, "kind": "file.open", "modal": true, "params": {…}}],
 "withdrawn": [{"ask": 17, "why": "the document closed"}],
 "committing": [],
 "log": {"seq": 22091, "worst": "warn"}}
```

Empty arrays MAY be left out. `log` says the ring has lines past what the shell last read and how bad the worst is; the shell reads them with `log.tail`.

**As built (C ABI, N1 and N2 slice 2):** `changed` carries `{doc, domain: "engine", gen, connected, label, ready}`, `{doc, domain: "document", closing: true}`, `{doc, domain: "menus", gen}` once the shell has called `menu.get`, and `{doc, domain: "scene", gen}` once it has called `scene.get`. The core looks at a document's scene when it is idle and at most every 16 ms while busy (a constant, `SCENE_EVERY`, not yet `display.frame_ms`), and a scene change not yet polled is replaced by the newer one, so a poll holds at most one scene change per document however fast a run reports. Since N2 slice 3 a look put off because the last was under 16 ms ago is taken when the 16 ms are up, not at the next 50 ms tick, so an edit or a geometry report reaches the feed and the hot calls' snapshot within 16 ms of the core going quiet. The egui editor's control port has no feed (it is request and reply); a script there calls `scene.get {since}` when it likes. **Since N3 slice 2** a `scene.get` answered in a turn in which the core took no look of its own counts as published for that turn only: before, the next look (after an edit in a later turn) took the edit's scene for the one already published, saw nothing new, and the edit reached the feed only at the next idle look, a tick (50 ms) and a build later. A shell that fetches `scene.get {since}` on every scene change, as the Swift kit's document model does, met it on every edit: an edit's change came 67 to 80 ms after its reply at 500 nodes, and comes within the 16 ms throttle and a build since (`crates/ffi/tests/scene.rs`, `an_edit_reaches_the_feed_without_waiting_for_a_tick`).

**Since N3 slice 4, the `document` domain carries the document's state** (7.2): once a document's engine is there, and whenever one of them changes, `changed` has `{doc, domain: "document", gen, title, document, path, dirty, undo, redo, lastError}` (`undo` and `redo` the step names, "Add Node", or null; since B71 also `installing`, `{repo, status}` while an engine install runs, else null; since B167 `errorSeq`, how many errors the document has said: it rises with every error, the same text again too, so a shell shows a banner when `errorSeq` moves and never decides by comparing texts; `ui.inspect` has it too). **Since research note 28 the document domain also carries `statusLine`**, the status line as the core's model (`plumcraft_editor_core::status_line`), which every shell draws and none composes: `{on, segments: [{id, role, side, priority, text, short?, detail?, detailTone?, tone, dot?, busy?, on?, command?: {id, params, label?}, tooltip?}]}`. `on` is View › Status Line (`view.statusLine`, kept in the preferences, on by default). Segments, left to right: `engine` (the short name, `dot` ok, busy or down, the long label as tooltip, tone `warn` with a manifest warning, the connected engine's own status text as `detail` in `detailTone`), `install` while one runs, `document` (the name, `•` when edited, the graph's size as detail, the path as tooltip), `selection` when something is selected, `hash`, `run` (running: the count, `busy`, the time as detail, `run.cancel` as command; after a run: its time, the engine's time and the cache as detail, `run.cacheReport` as command; or "not run yet"), `store` (a store refusing writes or the disk low, tone `warn`), `auto` (role `toggle`, `on`, `run.auto`), `save` while one is pending, `convert` (a refused wire's converter, `wire.convert`), then `message.0`… on the right side, most serious first, each opening the Messages window (`view.messages`), which stay until what they say changes. A shell draws the segments in order, a segment's `tone` in its theme's colours, a `busy` one with a spinner, a `toggle` lit when `on`; a click runs its `command`. A segment is shown whole or not at all, never cut: when the width runs out a shell drops the detail lines, then the segments of priority 3, then 2 (the last first), then shows the `short` forms (the engine's name, the run's time or count, the hash's algorithm, "Auto"), then drops priority 1 from the end; the newest message keeps its place throughout and is cut only when it alone is wider than the line (the Swift kit's `StatusLine.fit`, the Windows models' `StatusLine.Fit`). `ui.inspect` has it as `statusLine`, a newer one not yet polled replacing the older (`closing` changes stay as they were, with no `gen`). So a native shell keeps its window's title, its edited dot and its Edit menu's step names without asking (`crates/ffi/tests/run_watch.rs`). `dirty` is the session's: an undo back to where the file was saved leaves it dirty.

### 7.2 Domains

| Domain | What | Fetch | `ui.inspect` today |
|---|---|---|---|
| `document` | Title, name, path, dirty, undo and redo labels, closing, copied | `state.get` | `title`, `document`, `path`, `dirty`, `undo`, `redo`, `closing`, `copied` |
| `scene` | The canvas (section 8) | `scene.get` | `graph`, `groups`, `frames` |
| `view` | Requested transform (fit, reveal, `ui.set`), group focus and breadcrumb, highlight, palette | `state.get` | `canvas`, `groups.focus`, `popups.palette` |
| `selection` | Selected nodes, collapsed groups, net | `state.get` | `selection`, `groups.selected` |
| `run` | The run summary, run animation, save in flight | `state.get` | `run`, `runAnim`, `save` |
| `catalogue` | Node types, families, wire types, palette tree, Add menus by engine | `state.get` | `catalog` |
| `menus` | The menu model with enabled, reasons and checks | `menu.get` | (`command.list`) |
| `engine` | Engine state, choices, banner, engine status, installing | `state.get` | `engine`, `banner`, `engineStatus` |
| `watched` | The panel's watched value, viewer and its state, compare, sheet, `px`, pop-outs, wire peek | `state.get` | `watched`, `popouts`, `wirePreview` |
| `windows` | Which windows the document has open: parameters, value editors, settings tab, about, messages, acknowledgements, cache report, graph parameters | `state.get` | `parameters`, `editors`, `about` |
| `prefs` | Theme, tokens, appearance, text size, zoom, remembered folders | `theme.get`, `state.get` | `theme`, `prefs` |
| `status` | The status bar's cells and messages (14 in Appendix A) | `state.get` | `lastError`, parts of `run` |
| `asks` | Pending asks (also pushed in `asks`) | `state.get` | `prompt`, `rename` |
| `log` | Lines in the ring | `log.tail` | none |
| `pictures` | Derived picture refs that became ready (10.2) | `state.get` | none |

A domain's JSON is, field for field, what `ui.inspect` reports for it today where it reports it; `state.get` adds `gen`.

**N4 slice 4:** domain `prefs` (`{doc, domain: "prefs", gen}`): the core's look changed (a setting, the theme, the system's appearance; 14.2); the `document` domain's state carries `autosave` (4.4).

### 7.3 Guarantees

- **Coalescing.** A poll reports each changed `(doc, domain)` once, with its latest generation; intermediate generations are not delivered. The core holds `scene` and `run` changes to at most one per `display.frame_ms` (plan budget: one scene change per frame during a run).
- **Read your writes.** When a shell sees the reply to its request R, a `state.get` or `scene.get` it sends afterwards reflects R's effects. A reply and the changes it caused MAY arrive in the same poll; the reply never arrives before them.
- **Order.** Within a poll: `replies` in completion order, `changed` in no particular order, `asks` in the order raised. Across polls, a domain's generation never goes backwards.
- **No loss.** A shell that polls late misses intermediate generations, never the latest. Asks are never dropped; a withdrawn ask is listed in `withdrawn`.
- A shell MAY call `state.get` for any domain at any time, whatever the feed said.

## 8. The scene

**As built (N2 slice 2).** `plumcraft_editor_core::scene` builds the scene from the document (graph, catalogue, run state, the Watched panel's node, selection, group entered, the application's node lines and wire marks) and a theme; `scene::Published` keeps it with a revision per item. Rust types (`Scene`, `Face`, `InRow`, `OutRow`, `Widget`, `Preview`, `Footer`, `Wire`, `Net`, `Frame`, `Group`, `Mark`, `Delta`) are the wire shape, serde-encoded; the shared fixtures (16.2) are the reference. Where the subsections below still show the draft's shape, these differences hold:

- **Colours** (`paint`, `fill`, `wire`, a tint's `header` and `body`) are `{token}` or `{color: [r, g, b, a]}`, exactly one (N2 slice 4, question 13 as decided): a colour that is a theme token is sent by its name alone, and the shell resolves it with `theme.get` (6.3), which has every token a scene names (the Settings table's, and `pin_hidden`, `group`, `group_body`); a computed colour (a family's tint, an engine's wire-type colour, a frame's own) is sent as itself, premultiplied. So a theme change changes only the faces whose computed colours change. Slices 2 and 3 sent both, and every face changed with the theme.
- **Geometry (N2 slice 3):** a wire has `path` (8.8: a cubic chain between its pins; empty for a wire drawn through a net), an open group `rect` (`[x0, y0, x1, y1]`, its band the top 28), a mark `at` (its wire's middle), and the top level `view: {fit: {seq, rect} | null}` (8.9). They follow the sizes the shell reported (8.11), else a 220 × 160 face with its pins estimated (header 28, rows 22). Geometry is rounded to a thousandth of a graph unit and sent as `f64`, so its JSON reads back exactly. A face has no size in the scene: the shell measured it. Still none: `clock`, `grid`, `highlight` (a reveal's highlight is the shell's), `buttons`.
- **The header** is `{open, icon, about: {name, description, family?, path?, id, known}, state: {kind, paint?, hover?}, title: {text, watch}, badges: [{text, paint, hover}], engine: {text, pinned, hover}?, status: [{text, paint, strong, hover?}], enter}`. `title.watch` stands for the draft's tap (`run.watch`) and double tap (`node.parameters`); `enter` for a proxy's pencil (`canvas.focus`); the chevron is `open`.
- **A face** also has `run: {state, fraction, stale, notRun}` (the real state, 8.6's first layer, `state` one of `queued started progress preview done cached error skipped cancelled`) and `shown` (the animation's, a string). No `about` at the top level (it is the header's), no `highlight`.
- **An input row** is `{key, node, label, list, ty, pin: {shape, fill, wire, wireable}, source, text?, hover?, dim, from?, named?: {name, paint, hover}, badge?, widget?, inactive?, editor?, section?}`: `node` is the member a proxy's row stands for (the draft's `member`); `section` (FR5, a collapsed group's face only) is a header the shell draws above the row, the title of the node the promoted rows from there on come from, as that node's face shows it (it follows a rename of the node; one per source node, even when there is only one; for a group inside a group, the innermost node's, since a promotion always names the inner node's own port: assumed). A secondary click on a promoted row's caption is the row's own menu (`menu.get {kind: "input", port}`, from the core's gesture): Rename Parameter… (`name.edit {promoted}`, the group's name for it) and Use the Port's Own Name (`group.editPromoted {newLabel: ""}`); `from` is `inherited`'s "from <bundle>". An output row is `{key, node, label, list, ty, pin, hover}`; a tap watches `{node, port: key}`.
- **A widget** is `{kind, value, default, blank, clearable, min, max, softMin, softMax, step, decimals, int, choices: [{value, label, disabled?}], pathMode?, colorFormat?, hover?}`.
- **A path widget's Browse button (B133, 2026-11-23).** A row whose widget has `pathMode` (`open`, `save`, `dir`, `files`) and can be edited draws a Browse button: the square at the right end of the widget's rect as the shell reports it in `rows`, its side the rect's height, at most half its width (`geometry::browse_rect`; the shell measures nothing more). The rest of the field shows the path as text and edits it as before. A tap inside the square is the core's gesture `selection.set` then `row.browse {node, port}`, invoked (the core asks; see 11.2's `file.open`/`file.save`/`folder.pick` with purpose `path`); a tap elsewhere on the field is `row.edit`. The shell's glyph: the platform's own for choosing a file (a folder or an ellipsis), in the widget's text colour.
- **A preview** is `{kind: "rgba8", handle, w, h, size, hoverSize, items, levels?, summary, note?, faded}` or `{kind: "peaks", handle, channels, size, note?, faded}`; `handle` is the value's `DataRef` as JSON (what `media.get` takes). A text excerpt is a footer item, not a preview.
- **Footer items** are `fields {text, hover, all}`, `convert {lines: [{text, paint, hover?}]}`, `editor {label, hover}`, `progress {fraction, stage?}`, `error {text, full, paint}`, `save {port, sheet}`, `excerpt {text, faded, hover}`, `engineText {text, full}`, `appText {text, full}`, in that order.
- **A wire** is `{id, nid, rev, from, to, fromFace, fromPin, toFace, toPin, paint, kind: "plain" | "routed" | "net", via, slice?, net?, path}`: the faces and pin indices it is drawn between (a collapsed group's face stands for its members); `slice` is the label text (`[:-1]`).
- **A net** is `{id, nid, rev, key, paint, selected, driver: {from, face, pin, at}, uses: [{to, face, pin, at}]}`; `id` is `name` or `name@group`.
- **A group** is `{id, nid, rev, name, collapsed, parent?, depth, proxy, rect?}`; a frame `{id, nid, rev, title, pos, size, paint}`; a mark `{id, nid, rev, label, tone, paint, tooltip?, at?}`, `id` the wire's.
- **Top level:** `{shell, doc, gen, full, nodes, wires, nets, frames, groups, marks, selection: {nodes, groups, net}, focus: {group, path: [{id, name}]} | null, anim: {on, replay, generation, holdMs, fadeMs, pulseMs}, view: {fit: {seq, rect} | null}}`; `view` comes in every reply, as `selection`, `focus` and `anim` do.
- **Order is drawing order:** faces, wires and the rest come sorted by id (the published scene's order), and the hit test takes the last face as the one on top (9.2).
- **`nid`** is one counter for every kind of item in a document (faces, wires, nets, frames, groups, marks), so no two items share one, whatever their kinds; never reused within the document's life. A hit's `item` therefore names one thing. (Slice 2 reported that a face and a wire could share a number; since slice 3 the counter is shared, and slice 4 tests it: the conformance case "no two items of a scene share a nid".)

### 8.1 Coordinates

Everything in the scene is in **graph units**: `pos` and `size` of faces, frames and groups, wire points, `via` points, tag offsets. The canvas transform is the shell's (8.9); the scene never changes when the view pans or zooms. Today's constants in graph units: wire width 2.5 (`crates/ui/src/canvas.rs:59`), thumbnail box 80 (`run_view.rs:24`), hover thumbnail 160 (`:26`), the header's status slot 88 (`canvas.rs:61`). Hit slops are screen points today (wire 12, port 9, `canvas.rs:55-57`): the shell divides by zoom before a hot call (9.2).

### 8.2 Shape

```json
{"shell": 1, "doc": 1, "gen": 412, "full": true, "clock": 183920.5,
 "nodes": [<face>…], "wires": [<wire>…], "nets": [<net>…], "frames": [<frame>…], "groups": [<group frame>…],
 "marks": [<mark>…], "anim": <run animation>, "grid": {"token": "grid"}}
```

Every item has a string `id` (the graph's), a numeric `nid` (u64, stable for the document's life: an interner, never reused) used by hot calls, and `rev`, the scene generation at which it last changed. `clock` is the core's monotonic clock in milliseconds when the scene was published; times in the scene are on that clock, and a shell maps them by the offset at receipt.

### 8.3 A node face

```json
{"id": "n2", "nid": 7, "rev": 405, "kind": "text.upper", "proxy": null,
 "pos": [320, 0], "view": "full", "watched": true, "highlight": false,
 "tint": {"header": [214, 120, 98, 255], "body": [247, 236, 232, 255]},
 "border": {"token": "state_done", "width": 2.0},
 "header": {…}, "inputs": [<row>…], "outputs": [<row>…], "preview": <preview> | null, "footer": [<footer item>…],
 "shown": {"state": "done", "since": 183100.0}, "about": {…}, "menu": "node"}
```

| Field | Meaning (today) |
|---|---|
| `kind` | The catalogue's node type; unknown kinds still have faces (`canvas.rs:2154`) |
| `proxy` | For a collapsed group's node: `{group, name}`; its rows stand for members' ports (8.7) |
| `view` | `full` (pins, widgets, preview), `compact` (no preview), `collapsed` (title only) (`plumcraft_graph::NodeView`) |
| `watched` | The Watched panel shows this node: the border is 1.5 wider (`canvas.rs:1432`) |
| `highlight` | Revealed: a 3.0 border in `highlight` for 2 s (`canvas.rs:1426`, `:619`) |
| `tint` | The family colour: `header` from `Tokens::family_tint` (pinned fruit, else the engine's wire-type colour of the family's name re-lit, else the fruit the family hashes to), `body` mixed from it (`theme.rs:486`). A proxy uses the `group` tint |
| `border` | Highlight (3.0) > proxy (`group` header colour, 2.0) > stale (`stale`, 2.0) > the run state's colour (2.5 for error and running, 2.0 otherwise) > watched without a state (`accent`, 2.5) > none (`canvas.rs:1424-1444`). A token name, or `color` for a computed colour |
| `shown` | The run animation's state for the node (8.6) |
| `about` | The node type's name, description, family and its description, palette path and id, for the icon's tooltip and the hover card (`canvas.rs:2064`) |
| `menu` | The context menu kind to fetch with `menu.get` (12.4) |

**The header** (`canvas.rs:1759-1896`):

```json
{"chevron": {"open": true, "command": "node.view", "params": {"id": "n2", "view": "collapsed"}},
 "icon": "type", "state": {"kind": "dot", "token": "state_done", "hover": "done"},
 "title": {"text": "Upper case", "tap": {"command": "run.watch", "params": {"node": "n2"}}, "doubleTap": {"command": "node.parameters", "params": {"node": "n2"}}},
 "badges": [{"text": "output", "token": "accent", "hover": "group output: think it over › shout"}],
 "engine": {"text": "text •", "pinned": true, "hover": "Runs on text (pinned). …"},
 "status": [{"text": "stale", "token": "stale", "hover": "…"}, {"text": "0.02 ms", "token": "header_text"}],
 "buttons": [{"icon": "ellipsis-vertical", "menu": "node"}]}
```

- `chevron`: a proxy's opens the group (`group.collapse {collapsed: false}`); a node's toggles `node.view` (today through egui-snarl's open flag, reconciled to `node.view`, `canvas.rs:699-710`).
- `icon`: a Lucide name from `NodeDef::icon`, else `boxes` for a group, else the fallback (`crates/ui/src/icons.rs`). Shells ship the same Lucide set (ISC).
- `state.kind`: `spinner` in the running states (`started`, `progress`, `preview`), `dot` in the state's token otherwise (`Tokens::state_color`, `theme.rs:528`), `none` before any run (the slot is kept so the header is as wide).
- `title.tap` exists only when the node has outputs (`canvas.rs:1811`).
- `badges`: the export mark (`texts.export_mark`, `group.checkpoint`) and "output" for a group output (`groups_view.rs:671`).
- `engine`: only with several engines offering the kind; `•` when pinned (`canvas.rs:1829`).
- `status`: the core's words: `not run` (with its hover) when no run completed the node; `stale` with `staleWhy`; the cached text (`cached_text`), the time (`format_ms`), the running fraction (`running_text`), or `queued`/`skipped`/`cancelled` (`canvas.rs:1841-1864`). The shell keeps the slot at least 88 wide.
- `buttons`: the node menu; a proxy also has `{icon: "pencil", command: "canvas.focus", params: {group}}` (enter).

### 8.4 Rows

An **input row**:

```json
{"key": "sigma", "pin": 31, "label": "sigma", "list": false, "ty": "FLOAT", "pinShape": "circle", "pinColor": [96, 140, 220, 255], "wireable": true,
 "source": "own", "hover": "Type: FLOAT\nBlur radius",
 "named": {"name": "radius", "doc": "…"} , "dim": true, "inactive": null,
 "widget": <widget> | null, "text": null, "badge": null, "menu": "input",
 "member": null}
```

| `source` | Drawn as (today, `canvas.rs:1154-1231`) |
|---|---|
| `wired` | `text` = `label = value (wired from Title)`, no widget; with `badge` `{text: <bundle word>, command: "bundle.reset", params: {node, port}}` when a wire overrides a bundled value |
| `socket` | A socket-only input: `text` is the label with its type (`audio · WAV`) unless the label names it (`typed_label`). A pack's own type (`x.<pack>.<name>`, interface 7.2) is shown as `<name>` (`widget_text::shown_type`; S3 of an application's iPad app, 2026-11-23): `signals` on `x.<pack>.signals` reads `signals`, the full id staying in `hover` (`Type: …`). Output rows' labels the same |
| `inherited` | From the bundle wired into the node: label in italics, the bundle's value, "from <word>" |
| `own` | The widget, with the caption dim when the value is the default |
| (any) `editor` | `{kind, label, hover}` on a row whose value its node names an editor for (`NodeDef.editor.port`), while no wire feeds it (FR1, the maintainer 2026-11-23: "a row that opens it", for "every value an engine names an editor for"). `label` and `hover` are the application's registered editor's (egui), else "Edit Value…" and "Open the value (this app has no <kind> editor of its own)". The row opens it, `node.editor {node}` on the row's `node`: on a node's face its own (beside its footer), on a collapsed group's face the member's, for a value promoted by `group.promote` as any input is. What opens is what the shell has for the node: egui the registered editor (or "this application has no <kind> editor"), the C ABI shells the core's value window (the value as JSON, 12.5). The row keeps its words (`text`, the same short form as the member's own row). egui draws a button on a collapsed group's row (the member's footer has it on its own face); the Apple shells draw the caption in the `accent` colour, and a tap opens it (8.13) |
| `readonly` | A widget the editor doesn't know: `label: value`, or an application editor's summary. With no summary, a value longer than 40 characters is summed up (`widget_text::value_short`: an object as its fields' names, a list as its length, a string cut with `…`) and `hover` holds the whole of it (up to 4,096 characters): the native shells register no application editors, so a curve's JSON no longer fills the face |

- `pinShape`: `circle` for scalars, `square` otherwise; `pinColor` the wire type's colour, else the `pin_default` token; a non-wireable port has `wireable: false` and a hidden pin (`canvas.rs:1045`, `:1230`).
- `list: true` adds `[ ]` after the label (inputs) or before it (outputs).
- `named`: the net-label tag `name →` before the caption, for a named port (`canvas.rs:1053`).
- `inactive`: "Not used now: read only when …" (`widgets::inactive_note`), shown in the parameters sheet.
- `member`: on a proxy's row, the member's `{node, port}` that the row stands for.
- Variadic slots and a choice's sub-ports are rows of their own (`input_rows`, `canvas.rs:150`): `key` is `c2` or `mode.turn`.

An **output row**: `{key, pin, label, list, ty, pinShape, pinColor, hover, tap: {command: "run.watch", …}, menu: "output", member?}`.

### 8.5 Widgets

**N4 slice 4, numbers' text.** A number widget carries `text`, the number as every shell shows it (on the face, in the parameters inspector, while scrubbing): an INT whole, never with decimals; a FLOAT at `decimals` (`widget_text::float_decimals`: the port's `decimals`, its `step`'s, and never fewer than its default as written or the current value needs, the value read to a part in 10⁹ with at most 6 decimals, so a slider's `0.30000000000000004` shows `0.30` on a port of two decimals and float noise never shows). The egui sheet uses the same rule. An Apple inspector's slider shows its value live in the field beside it and sets it once, at the drag's end, on the widget's step and decimals; the inspector reads its rows again on every scene change and redraws only when they differ, so an edit on the face, a scrub, undo, redo or a script's `row.set` shows in it while it is open.

Every widget commits one `node.set {node, port: key, value}`, one undo step (`crates/ui/src/widgets.rs:3`): a drag on release, typed text when the field loses focus or on Return, a checkbox or a choice at once, a colour when the picker closes. A shell MUST NOT commit intermediate values.

```json
{"kind": "number", "value": 0.375, "default": 0.5, "blank": false, "clearable": false, "blankHint": "…",
 "min": 0, "max": null, "softMin": 0, "softMax": 4, "step": null, "decimals": 3, "int": false, "hover": "…"}
```

| `kind` | Catalogue fields that drive it | Canvas face | Parameters sheet |
|---|---|---|---|
| `number` | `ty` FLOAT or INT (`int`), `min`, `max`, `soft_min`, `soft_max`, `step`, `decimals` | A drag field; INT drag speed `step·0.1` (default 0.1); FLOAT speed `step` or `10^-decimals` (`widgets.rs:148-206`) | A slider over the span (`soft_min..soft_max`, else `min..max`) when finite, with the bounds at each end, `…` where typing may go past, and a typed field held to `min..max` (`widgets.rs:366`); else the drag field |
| `toggle` | `ty` BOOL | Checkbox | Switch, with "on"/"off" (`param_widgets.rs:36`) |
| `choice` | `choices` (or `ty` ENUM), `disabled_choices` | Combo, 120 wide | Segmented for at most 5, a searchable list for more (`params_view.rs:366-372`) |
| `multichoice` | `widget: "multichoice"`, `choices` or the choices of the input it feeds | A text field (comma-separated) | Chips, filtered past 16, "None picked: every choice" (`param_widgets.rs:119`) |
| `text` | `ty` STRING | One line, 140 wide | As wide as the sheet |
| `multiline` | `widget: "multiline"` | Monospace box, 3 rows, scrolls past 64 high; Return is a new line; ⌘Return runs after committing | 8 rows, up to 320 high |
| `path` | `path: {mode, filters}`, mode `open` (or empty), `save`, `dir`/`folder`, `files` | A text field, then `…` which asks (11.4); `files` is one path per line | Same, wide |
| `color` | `widget: "color"` (`r,g,b` in 0..1) or `"hexcolor"` (`#rrggbb`) | A swatch that opens the picker | Swatch and the text |
| `editor` | `NodeDef.editor {kind, port}` naming an application's value editor | A button with the editor's label; the footer carries it too | The button, beside the widget when the type has one |

Common fields: `value` (the current value, `ports::current_value`), `default` (the filled default when a choice fills it, `params_view.rs:233`), `blank` (the value is JSON null: the field shows "blank"), `clearable` (default null and current not: a `×` sets null, hover `texts.blank_value`), `choices: [{value, label, disabled?: reason}]`, `reset` (sheet only: `{value}` when current differs from default, `↺`). Values a shell does not understand are passed back unchanged.

### 8.6 Run state as shown

Two layers, both today's:

- **The node's state** (`NodeRun.state`, `crates/session/src/run.rs:38`): `queued`, `started`, `progress` (with `fraction`, `stage`), `preview`, `done`, `cached`, `error`, `skipped`, `cancelled`, or none; plus `stale` (an edit made a completed result out of date, `run_view::shows_stale`) and `notRun` (nothing has completed it). These drive the header, border and footer.
- **The animation's state** (`run_anim::Shown`, `crates/ui/src/run_anim.rs:32`): `waiting`, `running`, `done`, `cached`, `failed`, `skipped`. It differs from the real state during View › Animate Last Run (`replay`). A proxy shows its members' busiest: running, then waiting, failed, skipped, done, cached (`canvas.rs:1110`).

The fill while a run or its replay shows (`canvas.rs:1115-1136`), which the shell computes so the core never publishes per frame:

| Shown | Mixed into `tint.body` | Amount |
|---|---|---|
| `waiting` | `field` | 0.55 |
| `running` | `state_running` | 0.42 + 0.14·sin(2π·phase), one beat per 1.2 s |
| `done` | `state_done` | 0.40 |
| `cached` | `state_cached` | 0.24 |
| `failed` | `state_error` | 0.45 |
| `skipped` | `state_skipped` | 0.60 |

The amount is multiplied by **strength**: 1 while the run or replay goes on and for `HOLD` (1000 ms) after, falling linearly to 0 over `FADE` (600 ms) (`run_anim.rs:19-21`, `:225`). `anim` in the scene: `{on, replay, generation, endedAt: <clock ms> | null, holdMs: 1000, fadeMs: 600, pulseMs: 1200}`; each face's `shown: {state, since}`. With View › Colour Nodes While Running off and no replay, `on` is false and no fill applies. `mix` is per channel, alpha from the body (`theme.rs:380`). The thumbnail is faded (`faded` token tint) while stale, from an older run, or ahead of the replay (`canvas.rs:1140`).

**As built (N3 slice 4), over the C ABI.** The run animation is the core's document's (`Document::run_anim`, brought up to date every turn by `Document::pump_run_anim`; the egui shell draws from the same one). A scene the C ABI serves carries it while it shows: `anim.on` is true from the run's start (or a replay's, View › Animate Last Run) until the colours have faded (`HOLD` then `FADE` after every node is shown finished), and each face's `shown` is the state as shown (a string); after that `anim.on` is false and no face has `shown`, so a scene at rest carries no animation. With View › Colour Nodes While Running off (and no replay) it is never on. The scene's `anim` is `{on, replay, generation, hold_ms, fade_ms, pulse_ms}`, as the scene types serialise it; `endedAt` is not sent: a shell starts its hold and fade when no face is shown `waiting` or `running`, which is when the core's own clock starts them, and the core drops `on` at about the time the shell's fade ends. A replay's states change with time, not events: the core's idle look (at most every 50 ms) publishes them. Before, the C ABI's scenes had no animation (`on` false always).

### 8.7 Previews and footers

`preview` (only in view `full`, `canvas.rs:1629`):

```json
{"kind": "rgba8", "handle": "md5:nd1:…", "w": 640, "h": 480, "size": [80, 60], "items": 12,
 "summary": "array HSI 64×80 float32", "note": "From the previous run: …", "faded": false, "missing": false,
 "tap": {"command": "run.watch", "params": {"node": "n2"}}, "hoverSize": [160, 120]}
```

`levels` (absent when empty; nested lists, interface 7.6 rule 2) is a list of lists' count at each level, outermost first (`[3, 2]`: 3 runs of 2 items each; `items` is then the runs, `levels[0]`): a shell writes it on the face as `3 × 2` where it writes `items` otherwise ("3 × 2 items", "Lab · 3 × 2 pictures"; decision 6C for the face). A pop-out of such a value has two steppers (decision 6A): the list's own over the runs ("run 2 of 3", or the run's label from the sheet), and one a level inside the run ("item 1 of 2"; a level between says "run"), which sends `run.item {index, path}` with only its own level changed (egui `watched::level_steps`, Apple `LevelSteppers`, Windows `ListNames.Steps`).

`caption` (absent when empty) is the picture's own name, the rendition's: with `items` above 1 the value is a list of named pictures and this is the first of them (an engine's view of a value as several named pictures, each in its own mapping, through the list's stepper and sheet, interface 10.2). The Apple shells write `<caption> · <items> pictures` (`<items> items` with no caption) on the face's picture (`LayerCanvas.listTagText`), and the pop-out's stepper names each item by its `sheet` label with a menu of all of them (`ViewerBar.itemNames`), as the egui editor's list of items does.

`kind` `peaks` (a waveform, `{handle, channels}`; decoded with `media.peaks`, drawn in `waveform` on `waveform_bg`, `audio::THUMB_SIZE`). `missing`: the bytes were reaped; the shell shows "re-run to refresh" on `placeholder`. A shape the core can't read is `{"kind": "unreadable", "text": "preview unreadable: …"}`.

Footer items, in order (`canvas.rs:1267-1390`):

| `kind` | Fields | Action |
|---|---|---|
| `fields` | `text` ("3 more fields" or "Fewer fields"), `hover` | `node.fields {node, all}` |
| `convert` | `lines`, `warn` (a step isn't lossless) | none |
| `editor` | `label`, `hover` | `node.editor {node}` |
| `progress` | `fraction` 0..1, `stage` | none |
| `error` | `text` (cut to 180 chars), `full`, `token` (`text_dim` for skipped, `error` otherwise) | none |
| `save` | `port`, `sheet`: Save Result…, and Contact Sheet… on the right half (when items > 1) | `run.save {node, port, sheet, ask: true}` (11.4), done by the core on a tap (`ui.gesture`; the shell measures the line as `parts.footer[k]`); a menu item carrying these params may be run with `command.run` too: the core invokes it, as `ui.invoke` does |
| `excerpt` | `text` (3 lines, 200 chars), `faded` | `run.watch {node}` |
| `engineText` | `text` (120 chars), `full` | none |
| `appText` | The application's overlay line (160 chars), `full` | none |

### 8.8 Wires, nets and marks

A wire is keyed by its input (one wire per input), as `WireMark`s are (`commands.rs:65`):

```json
{"id": "n3.text", "nid": 12, "rev": 398, "from": {"node": "n2", "port": "text"}, "to": {"node": "n3", "port": "text"},
 "fromPin": 31, "toPin": 40, "color": [96, 140, 220, 255], "width": 2.5,
 "path": [[x, y], …], "via": [[120, 40]], "slice": {"text": "[:-1]", "at": [x, y]} , "net": null, "hidden": false}
```

- `path` is a cubic Bézier chain: the start point, then three points per segment. Today a plain wire is egui-snarl's degree-5 Bézier through six control points (`run_view::wire_points`, `crates/ui/src/run_view.rs:286`) and a routed wire a Catmull-Rom spline through its `via` points, drawn as cubics, horizontal at both pins (`wire_route.rs:8`). Native toolkits draw cubics, so the core converts (open question 7). The core hit-tests the same curve it sends. **As built (N2 slice 3), four cubics, not two:** egui-snarl's degree-5 curve (ported to the core in full, `wire_path::snarl_quintic`, its frame `min(32.4, length / 6)`) split evenly in four and each piece replaced by the cubic with its ends and end tangents. Two pieces strayed up to 2.2 graph units from the quintic on steep wires; four stay under 0.35 at usual lengths and 0.72 on a 5,000-unit wire (`claude-debug/native-n2/s3-fit-check.py`). A plain wire's path is 13 points, a routed one's 3 a segment (exact: the Catmull-Rom curve is cubic already).
- **A wire that runs right to left (N4 slice 2; the maintainer, N3 exit card 13: "Lay out engine could also add bend points on wires going right to left so as to clear the corners of the sending and receiving nodes").** A plain wire whose input is left of its output (`to.x < from.x`), with both faces' rects known, is routed by the core (`wire_path::backward_chain`): out of its output to the right by 24 (`BACK_CLEAR`), along a channel, and into its input from the left, the corners rounded (radius 12, less on a short leg). The channel runs midway between the faces when one is wholly above the other; otherwise (a node beside or directly behind) below both or above both, 24 clear, whichever is the shorter way. Its `path` is then up to nine cubics (28 points) rather than four: the format is the same (a cubic chain of any length; `plum_wire_paths` gives each wire's point count), so the ABI is unchanged. The hit test, a wire's middle (marks, `geometry.locate`) and `plum_wire_paths` during a drag (with the moved faces' rects) use the same chain. The kind stays `plain`; a wire with bend points keeps its own route. The egui editor draws its plain wires with egui-snarl and does not draw this route (it hit-tests the core's chain); see 12.11.
- `via`: bend points; bend points that line up (at least two, each next one straight across from or above or below the one before) make a right-angled route, drawn with straight legs and rounded corners, leaving and entering its pins across (B155; the core's path already is). A shell draws a handle on each (radius 5, `wire_route.rs:32`) for drag (`wire.route`), double-tap to remove one, and the menu's Add Bend Point and Straighten.
- `color`: the wire type's colour, else the `wire` token (`wire_route::wire_color`).
- `net`: the net's key when the wire is drawn through a net label; then `hidden: true` and the net carries its tags.

A **net** (`crates/ui/src/net_view.rs`): `{key, name, color, selected, driver: {pin, at: [dx, dy], text: "picture ×3"}, uses: [{to, pin, at: [dx, dy]}]}`, `at` the tag's offset from its pin (default `net_view::default_use_at`). Tags: tap selects the net (faint lines to every use), double-tap goes to the driver or the next use, drag moves (`wire.labelAt`, `net.move`), drop on an input wires it from the net (`net.connect`, ringed in `highlight` when it fits and `error` when not).

A **mark** (`Hooks::wire_marks`): `{to: {node, port}, label, tone: "info" | "warn" | "error", tooltip, at: [x, y]}`, drawn on its wire, outlined in the tone's token (`accent`, `warning`, `error`).

### 8.9 Frames, groups, focus, view

A **frame** (`crates/ui/src/frame_view.rs`): `{id, nid, rev, title, pos, size, color: "#rrggbb" | null (the accent), holds: {nodes, groups}, menu: "frame"}`. Drawn under the nodes; its tab (☰ menu, icon `frame`, title) sits outside its top edge; the corner handle resizes (`frame.resize`); dragging the title moves it with what it held when the drag began (`frame.move`). Swatches are `frame_view::SWATCHES`.

An **open group's frame**: `{id, name, rect, depth, menu: "group"}`, computed by the core from its members' and nested proxies' measured rects (so it needs geometry), deeper groups first (`groups_view.rs:771`). Its band has a collapse chevron (`group.collapse {collapsed: true}`), the group icon, the name (double-tap renames), enter (`canvas.focus`) and a menu; dragging the band is `group.move`.

**Focus** (the group entered, `canvas.focus`; the document's since N2 slice 1, `Document::focus`): `view.focus: {group, path: [{id, name}]}` for the breadcrumb ("Graph › outer › inner", each a `canvas.focus`; Leave is `canvas.focus {group: null}`). While focused only the group's members are in `nodes`.

**As built (N4 slice 3), the scene's `view` also carries** `select` (Select mode, 12.18), `reveal` (12.16), `windows` (the parameters inspector and pop-outs, 12.14) and `name` (a name edited in place, 12.16), each left out while off or empty. **Since N5 slice 3** also `zoom: {seq, by?, to?}`, the latest `canvas.zoom` (12.13), left out until one is asked: the shell zooms its view by `by` or to `to` about its centre, once per `seq`.

**The view domain**: `{target: {zoom, pan, seq} | null, fit: {seq, rect} | null, reveal: {node, seq} | null, palette: {open, query, selected, at, results} | null}`. **As built (N3 slice 1): `view.palette: {at: [x, y] | null, query?, wire?: {from: {node, port}} | {to: {node, port}}}`** while the palette is open, left out while it is closed (12.7). The core asks for a transform (after `graph.new`, `graph.open`, `graph.tidy`, `ui.set`, View › Fit, a reveal); the shell animates there and reports back with `ui.viewport`. `fit.rect` is every face's rect (and frames' outside a focused group) expanded by 32, zoom clamped to 0.2..1 (`canvas::fit_transform`, `canvas.rs:318`); the shell fits it to its own rect. **As built (N2 slice 3):** only `fit`, as the scene's top-level `view: {fit: {seq, rect}}`, set by `canvas.fit` (whose reply carries it too), computed from the core's geometry; `seq` rises with each request, so a shell fits once a request. **Since N2 slice 4** `graph.new` and `graph.open` owe a fit when they leave something to show: the `geometry.set` (or `plum_geometry_set`) that measures the last face shown asks for it, once, in `view.fit`. The egui shell measures in process without `geometry.set` and fits those itself, waiting for its sizes, as before.

### 8.10 What hover does today, and what replaces it

| Hover today | Where | Without hover (proposed) |
|---|---|---|
| A wire's or port's value as a preview after 250 ms, and `view.peek` after 500 ms | `wire_view.rs:19-21` | Tap a port or wire selects it and shows a popover with its preview and Watch (plan 5.5) |
| The node type's card on the icon or body | `canvas.rs:2051` | Long press › About, or the inspector |
| A thumbnail at 160 | `canvas.rs:1705` | Double-tap opens the value (`view.popout`) |
| Tooltips: types, help, disabled reasons, stale reasons, hash source | everywhere | The same text as an accessibility hint and in long-press menus; disabled reasons in menus as the item's subtitle |

### 8.11 Geometry: what the shell measures

The shell lays each face out from its rows at zoom 1, in graph units, and reports:

```json
{"faces": [{"node": "n2", "rev": 405, "size": [214, 162],
            "pins": {"in": {"sigma": 41.5, "mode.turn": 63.0}, "out": {"text": 118.0}},
            "header": 28.0,
            "rows": [{"key": "sigma", "rect": [96, 32, 210, 52]}]}]}
```

- `pins` are the pins' heights from the face's top; pins sit on the edges (egui-snarl `PinPlacement::Edge`, `canvas.rs:909`), so inputs are at x 0 and outputs at x `size[0]`.
- `rows` (capability `geometry.rows`) are widget rects in face coordinates, for the field-editor pattern and `ui.inspect`'s `parameters.fields`.
- A shell MUST report after a face's `rev` changes and after the text size or theme changes, and SHOULD report all faces in one call.

The core computes from them: wire endpoints and paths, hit tests (9.2), Tidy's `sizes` and `pins` (today `Canvas::measured_for_tidy`, `canvas.rs:446`), Align, Distribute and Frame Selection's `sizes` (`edit_view.rs:329`), open groups' frames, the fit rect, and `ui.inspect`. Until a face is measured the core assumes 220 × 160 (`canvas.rs:602`) and `graph.tidy` estimates as `command.run` does today.

**As built (N2 slice 3, `crates/editor-core/src/geometry.rs`):**

```json
{"faces": [{"node": "n2", "rev": 405, "size": [214, 162], "header": 28.0,
            "pins": {"in": [41.5, 63.0, null], "out": [118.0]},
            "rows": [[96, 32, 210, 52], null, null],
            "parts": {"title": [24, 4, 150, 24], "menu": [190, 4, 210, 24], "footer": [[8, 140, 206, 158]]}}]}
```

- **Pins and rows are lists in row order**, the order of the face's `inputs` and `outputs` (what a wire's `fromPin` and `toPin` index), not maps by key: a collapsed group's rows can share a key (two members' `text`). `null` for one not measured (estimated).
- `parts` names the header's parts (`title`, `chevron`, `icon`, `menu`, `enter`, `preview`) and the footer's items, for the hit test's parts (9.2); unknown names are refused. **Since N4 slice 2** a shell that reports typed (`plum_geometry_set`, which has no field for them) sends them after it with the command `geometry.parts {faces: [{node, parts}]}`, which merges them into the face's entry; a later report with no parts keeps them while the face's size and header are the same. `geometry.locate` takes `row: k` (the middle of input row `k`'s widget) and a part's name (its middle); `part: "header"` is the header's middle, or the nearest point along it that no measured part covers.
- The core keeps one entry a face (`Measured`), forgets faces gone from the scene and everything on `graph.new` and `graph.open`, and rebuilds the scene's paths and the hot calls' snapshot when it changes.
- What uses it: the scene's wire paths, open groups' rects and marks' places; `canvas.fit`; `selection.align`, `selection.distribute` and Frame Selection (`ui.invoke frame.add`), which pass the measured faces' sizes (an unmeasured one is left to the session's estimate); Tidy's measurements on a shell that reports (the egui shell's and the C ABI core's `Host::measured`); the hot calls.
- **The egui shell reports every frame**, in process (no command): each drawn face's size and pins as the canvas laid them out, and (since N2 slice 4) its `offset`. It doesn't report headers, rows or parts yet.
- **`offset: [dx, dy]`** (N2 slice 4) is where the face's top left is drawn, from its position; default `[0, 0]`, which is what a native shell drawing its face at its position sends (the typed `plum_face` has no offset). egui-snarl draws a frame's margin around the position and its pins on the frame's edges, so the egui shell's faces are the frames and say where they are; until slice 4 the core put them at the position, about 7 graph units off what egui drew (the conformance suite found it: a click on a face's top edge hit nothing).
- **Decided for Draft 2, not built: measured tags, frames' tabs and groups' bands.** The core still estimates them (9.5). A shell lays their text out, so it could report them; but they are drawn from the scene's data alone (a tag's name, a frame's title, a group's name), the estimates are conservative boxes at most one text line off, and no hit test there has been wrong in the suite or the egui shell's tests. They join `geometry.set` (as `tags`, `frames`, `groups`, each a list of rects by id) when a native shell's hit tests need them. Headers and parts are reported today (`header`, `parts`).

### 8.12 Revisions and deltas

`scene.get {since: g}` returns `{gen, full: false, upsert: {nodes, wires, nets, frames, groups, marks}, remove: {nodes: [ids], wires, nets, frames, groups, marks}, anim?}`: items whose `rev > g`, and ids gone since `g`. The core keeps removals for the last 256 generations; older, or no `since`, gets `full: true` with every item. Within an upserted item every field is present (no field-level patches). A shell redraws a face only when its `rev` changed.

As built (N2 slice 2): the delta also carries `shell`, `doc` and `since`, and `selection`, `focus` and `anim` come in every reply, full or not (they are small). `scene.get` rebuilds the scene and compares each item's JSON with what was published (a hash); the generation rises only when something changed, so asking twice with nothing changed gives the same `gen` and an empty delta. A `since` newer than the scene's `gen` also gets `full: true`. `remove` lists every kind, empty or not. Cost at 500 nodes and 800 wires (release, Apple M4 Max, `crates/ffi/tests/measure_scene.rs`): a delta after one node's run state changed is 2 KB and 4.5 ms request to reply, nearly all of it the rebuild and the hashing; the whole scene is 1.1 MB and 25 ms. **Since N2 slice 3** each item is compared with what was published as built (its typed value) rather than by hashing its JSON, every node's title is worked out once a build (it was a scan of every node for each), and the C ABI no longer builds a scene twice in a turn: a one-node delta is 2.4 ms at 500 nodes and 6.3 ms at 2,000 (36 ms before), wire paths included; the whole scene is 1.3 MB in 18 ms at 500 and 5.2 MB in 76 ms at 2,000.

### 8.13 Gestures and the commands they become

What a recognised gesture means is the core's (`crates/editor-core/src/gesture.rs`); every shell sends it as `ui.gesture` (6.3) and a script drives the same table. "Hit" is section 9.2's result. **As built (N2 slice 4):**

| Gesture on | Commands, in order |
|---|---|
| Tap a face's body, header, title, row, preview or footer | `selection.set {nodes: [id]}` (a collapsed group's face: `{groups: [g]}`); Shift `mode: add`, `primary` `mode: remove`. A title, preview or footer item also `run.watch {node}` when the node has an output (only where the shell reported those `parts`), except the footer's `save` line: its tap is `run.save {node, port, sheet, ask: true}`, invoked as the menu item is (the core asks `file.save`, then the format's options), `sheet: true` on the line's right half when it offers Contact Sheet… (since the Save Result branch: a core gesture, so no shell code; a shell's own `ui.gesture` is its person's, so the shell shows the asks, the control port's a script's) |
| Tap a header's chevron, enter button | `node.view` (full ↔ collapsed) or `group.collapse {collapsed: false}`; `canvas.focus {group}`. No selection change |
| Tap an output pin; a wired input pin | `run.watch {node, port}` of the output; of the wire's source. An unwired input: nothing (its row shows its value). No selection change |
| Tap a wire | `selection.clear` (unless Shift or `primary`), then `run.watch` of its source |
| Tap a net's tag | `net.select {name, group?}`, or `net.select {}` when that net is selected |
| Tap a frame's tab | Nothing (the tab's menu and title are the shell's controls) |
| Tap empty canvas, a bend point, a frame's body, a group's band | `selection.clear` (unless Shift or `primary`); on empty canvas also `net.select {}` when a net is selected |
| Double tap a face | `node.parameters {node}`; a collapsed group's: `canvas.focus {group}` |
| Double tap a wire; a bend point; a driver's tag; empty canvas | `view.popout {node, port}` of its source; `wire.route` without the point; `canvas.reveal` of the driver; `canvas.palette {open, at}` |
| Context (secondary click, Control-click, long press) | No command: `context` (what `menu.get` takes: `node`, `proxy`, `input`, `output`, `wire` (a bend point's too; it carries `at`, the press in graph units, so the menu's Add Bend Point puts the point there whether the shell takes the reply's `menu` or asks `menu.get` with this `context`, B153), `net`, `frame`, `group`) and `menu`, the menu model for it. On empty canvas: `canvas.palette {open, at}` |
| Drag a face | `begin`: the faces moving: the whole selection when the face is in it (selected groups not inside another selected one, and the selected nodes their groups don't carry), else that face alone, and the selection stays. `end`: one `edit.move {nodes, groups, drag: "gesture:<serial>"}` by the offset (since N3 slice 1; it was a `group.move` or `node.move` a face), one undo step; nothing when it didn't move |
| Drag an output pin, or an unwired input pin | A new wire. `move`: the target (`geometry.wireTarget`'s, `fits`). `end` on a pin: `wire.connect {from, to}` (refused, with the reason and a Convert offer, where it doesn't fit); on nothing (since N3 slice 1, question 8): `canvas.palette {open, at, wire: {from: output} | {to: input}}`, the palette there for what takes the wire (12.7); nothing when the pointer didn't move |
| Drag a wired input pin | The wire's end picked up (question 17, every platform). `end` on another input: `wire.reroute {to, newTo}`; on its own input: nothing; on nothing: `wire.disconnect {to}` |
| Drag a bend point; a frame's tab; a frame's handle; an open group's band | A bend point's `move` (since B158): `update.wire`, as for a route below. `end`: `wire.route` with the point moved; `frame.move` (with what it holds); `frame.resize`; `group.move` (the last three naming the drag, `drag: "gesture:<serial>"`) |
| Drag a right-angled route's corner or leg (B155) | The hit is a `wire` with `part` `corner` or `segment` and `index` (its corners counted from the output pin, 0): the corners of a wire running right to left routed round its faces, or of a wire whose own bend points line up (`wire_path::right_angled`), not the legs that leave and enter its pins. The drag is `route` (code 9). `move`: `update.via`, the wire's bend points as they would be (a corner moves the corners beside it on the axis they share; a leg moves across itself). `end`: one `wire.route` with them: an automatic route's corners become the wire's own, one undo step. A shell shows that it can be taken hold of (the Mac: the open hand over it) |
| The wire follows a bend drag (B158) | Each `move` of a bend point's drag (code 4) or a route's (code 9) also answers `update.wire: {nid, via, path}`: the wire's scene nid, its bend points and its path (a cubic chain, as the scene's `path`) as they would be with the pointer there, built by the scene's own path code, so it is the path the scene holds once the `end`'s `wire.route` has run. The shell draws that path in place of the wire's own, with dots at `via`, until the drag ends (keeping it until the scene brings the wire's new path, so it doesn't jump back) or is cancelled (the wire's own back at once). It computes no route of its own. Typed: `plum_drag_wire` (9.6). No other drag carries `wire` |
| A wire's bend points (B158) | A shell draws a wire's bend points only while the wire is under the pointer (or one of its bend points is), has an end on a selected face, or is being bent by a drag; small (the Apple canvas: a dot of radius 3 graph units in the wire's colour), never the rings of radius 5 drawn on every routed wire before. The hit test is unchanged: a bend point is hit within 5 graph units plus the slop, drawn or not, so a touch shell finds it on a wire it can't hover |
| Drag empty canvas, a wire, a tag, a frame's body | A marquee (on touch the shell pans there instead, and sends nothing). `move`: its `rect` and the faces it `touches`. `end`: as a marquee |
| Marquee | `selection.set {nodes, groups}` of every face it touches: plain `replace`, Shift `add`, `primary` `remove` |

- **Since N4 slice 2 (inline editing, 12.10):** a tap on a row (`part` 8) that can be edited: `selection.set` of the face, then `row.edit {node, port}` (the field editor opens on it), or for a switch `row.set {node, port, value: !value}`; Shift or `primary` held: the selection alone. A double tap on such a row: the same `row.edit`, not `node.parameters`. A tap on the more-button (`part` 6, `menu`): no command, the reply's `context` and `menu` the node's (as a context gesture). A drag that begins on a number's row: a **scrub** (`plum_drag` kind 8): `begin` replies `drag: {kind: "scrub", row: {node, port, face, index}, value}`; each move's value is the start moved by `(dx − dy) · zoom` screen points at egui's speeds (an INT `step · 0.1` a point, 0.1 with no step of 1 or more; a FLOAT `step`, else `10^-decimals`), held to `min`..`max`, whole for an INT, else rounded to the decimals shown; `ui.gesture` takes `zoom` (screen points a graph unit, default 1, 0.01 to 100) for this; `end` is one `row.set` (none when the value came back to where it began); no autoscroll. A drag on any other row moves the face, as before. The decision is the core's, so no shell decides what a press on a row is.
- **Since FR1 (2026-11-23):** a tap on a row whose scene row carries `editor` (8.4: the value its node names an editor for, not on a wire; on a collapsed group's face, a promoted one, the member's) is `selection.set` of the face, then `node.editor {node}` on the row's `node`, as the node's footer does; it comes before a row's `row.edit`. A shell reports such a row's caption as its row rect when the row has no widget (the Apple shells' `FaceLayout.opens`), so the hit test finds it; **since B173** a row with a widget too is reported from its caption to its widget's end, one rect, so a tap on the caption (drawn as a link) opens the editor, and a path row's Browse square, the rect's right end, still browses. Shift or `primary` held: the selection alone. **Since B173 the C ABI's documents pass the core's own editors** (`scene::core_editors`: `mask`, "Edit Mask…", the general mask editor; `curves`, "Edit Curves…"), so a node naming one has its row's words and the node menu's item (`menu.rs` `node_menu`) on every native shell, which draws the editor natively (the Swift kit's `NativeEditors`) or as the core's value window.
- **Since N4 slice 3 (12.16):** a double tap on a face's preview or footer (a node with an output), on an output pin or a wired input's pin is `view.popout` of the value; on a frame's tab `name.edit {frame}`; on an open group's band `name.edit {group}`; a tap on the band's chevron (its first 26 graph units) is `group.collapse {collapsed: true}`; a context gesture on a wire gives a menu whose Add Bend Point puts the point there.
- **A double tap opens what its first tap hit (after N4, egui's parity work):** a `doubleTap` within 0.8 s of a `tap` (`gesture::DOUBLE_TAP`) and within the hit distance of it (at least 4 graph units) is hit-tested as that tap was, not against the snapshot after it. The first tap can change the faces: a tap on an output pin watches it, the run that follows can widen the face (its run status grows the header), and the pin moves out from under the second tap, which then opened the node's parameters. A double tap with no such tap before it is hit where it is, as before. Every shell gets this from the core (the Apple apps with their next build of the core).
- **A cancelled drag** (`phase: cancel`) runs nothing; the document is as it was (the suite compares `graph.get`, `dirty` and the undo label before and after).
- **Undo:** a drag runs at most one command that is an undo step: `edit.move` (a face drag), `frame.move`, `frame.resize`, `group.move` (an open group's band), `wire.connect`, `wire.reroute`, `wire.disconnect` or `wire.route`. **Since N3 slice 1 moves are undo steps** (the maintainer's N2 exit decision 3; 12.6 has the rule): a face drag is one step named for what moved ("Move Node", "Move 3 Nodes"), so one `edit.undo` puts it back. A drag's serial rises with every drag begun (before N3 slice 1 it restarted at 1 once a drag ended), and its moves name it, so the next drag is a step of its own. A new wire dropped on nothing adds no step until a kind is picked from the palette (`palette.choose`, one step); dismissing the palette leaves nothing.
- **The egui shell** sends its clicks (a tap) and its Shift-drag marquees as `ui.gesture` (since slice 4; snarl's private selection is no longer read). egui-snarl owns node drags, wire drags and double and secondary clicks there, so those are still egui's own code (plan section 17, slice 4, lists them; the maintainer's N2 exit decision 9 keeps it so). **Since N3 slice 1** the egui shell's node, frame and group drags send one `edit.move` (or `frame.move`, `frame.resize`, `group.move`) a frame naming the press (`drag: "egui:<press>"`), so each drag is one undo step however many frames it moves things in; and a new wire egui-snarl lets go on empty canvas (its `has_dropped_wire_menu`) opens the core's palette for it, egui's palette then offering only what takes the wire and picking through `palette.choose`.
- **The egui shell's double clicks (after N4, the audit's D7; N3's card 3, "the same in every shell"):** a double click on the canvas is sent as `ui.gesture {kind: "doubleTap", at, mods, slop}` (egui counts a click within 0.6 s of the one before last as a triple and then no double: that is a double tap too), and the core's table above decides: empty canvas the palette there, a face its parameters, a collapsed group the group, an output or wired input pin, a wire, a preview or footer its value popped out, a bend point removed. egui's own double-click code for faces, titles, wires and bend points is gone. Two places take the click from the canvas and send the command the table names themselves: a frame's tab (`name.edit {frame}`) and an open group's band (`name.edit {group}`). A double click in a face's own field (a word selected, a value typed) is the field's, not a double tap. A net's tag keeps egui's own double click (to the driver, then each use in turn), richer than the table's driver-only `canvas.reveal`.

## 9. Hot calls

### 9.1 Wire paths

`plum_wire_paths(doc, nodes, n, dx, dy, out, cap) → size_t`: the paths of every wire with an end on one of `nodes` (by `nid`), as if those nodes were moved by `(dx, dy)` graph units. Output, in floats: per wire `wire_nid` (low 32 bits, then high 32 bits, each a `u32` bit-cast: since N2 slice 4; Draft 1 had one float) and `count` (u32 bit-cast), then `count` points as `x, y`, a cubic chain as in 8.8. Returns the floats needed; if that exceeds `cap` nothing is written. Bounded to a few microseconds for 50 wires: no allocation beyond the snapshot, no JSON.

### 9.2 Hit test

`plum_hit_test(doc, x, y, slop) → plum_hit`: what is at `(x, y)` in graph units, with `slop` the hit distance in graph units (the shell's target size divided by zoom: 9 points for a pin and 12 for a wire today, 44 points on iOS, 48 dp on Android). Order, from today's code: a pin first, since a pin sits on its node's edge (`canvas.rs:744`); then a widget row, header part or node; a bend handle; a net tag; a frame tab or handle; a wire; a group frame's band; a frame's body; empty canvas.

| `kind` | Fields |
|---|---|
| 0 none | |
| 1 node | `node`, `part`: 1 body, 2 header, 3 title, 4 chevron, 5 icon, 6 menu button, 7 enter button, 8 row (`index`), 9 preview, 10 footer item (`index`) |
| 2 pin | `node`, `pin`, `part` 1 input or 2 output |
| 3 wire | `wire` |
| 4 bend | `wire`, `index` |
| 5 net tag | `wire` (a use) or `index` (a driver, by net order) |
| 6 frame | `index`, `part` 1 body, 2 tab, 3 handle |
| 7 group | `index`, `part` 1 band, 2 chevron, 3 enter, 4 menu |

### 9.3 Wire target

`plum_wire_target(doc, from_pin, x, y, slop) → plum_hit`: the nearest pin within `slop` that a wire from `from_pin` may land on, kind 2, with `fits` 1 when `wire.connect` would take it, 2 when only through a Convert node, 0 otherwise. The type check is the session's (`wire.connect`), precomputed per pin pair in the snapshot.

### 9.4 Threading

Hot calls read the latest published snapshot (scene positions, geometry, pin table, compatibility) through an atomically swapped `Arc`, from any thread, without locks. They may be one generation behind a command just sent; their results never refer to items the snapshot lacks. They never mutate.

### 9.5 As built (N2 slice 3)

- **The snapshot** (`geometry::Snapshot`) is made on the document's thread after a publish, from the published faces, wires, nets, frames and open groups (with their nids) and the geometry: faces' rects, pins with their types, list-ness and the graph node they stand for, wires' chains and bounds, tags, frames' tabs and handles, groups' bands, and a table of which output type fits which input type (1, or 2 through a Convert, by `SchemaReply::accepts` and `convert_paths_where`). It is made again only when the session's generation, the graph's revision, the geometry or the group entered changed, so a run's progress doesn't rebuild it. The C ABI keeps each document's behind a mutex held only to clone or swap the `Arc` (not lock-free, but a reader never waits for work); the JSON forms publish first and read the document's own.
- **9.1, as built:** a wire drawn through a net has no path and is left out; bend points stay where they are when both ends move. Slice 3 cut nids to their low 32 bits; slice 4 sends both words.
- **9.2, as built:** one `slop` for pins, wires and tags (the shell picks its target size); a bend point takes `max(slop, 5)`; a pin must be wireable (a hidden pin isn't hit); the topmost face is the last in the scene's order; a face's part is a named part it reported, else a row's rect, else header (above `header`, default 28) or body; tags, frames' tabs (180 × 22 at most, above the top edge) and open groups' bands are estimated by the core (their text is the shell's to measure, and `geometry.set` doesn't carry them yet), only the band is a group's part (its buttons are the shell's controls); a point that isn't a number hits nothing.
- **9.3, as built:** among the pins in reach, one that fits beats a nearer one that doesn't; from an input the targets are outputs. `fits` is 0 for a pin that can't be wired (hidden, or a named port), the same node, a cycle (the snapshot keeps what each node feeds), or a single value into a list input; a wire to or from a Convert node shows as fitting (its own rules are `wire.connect`'s, at the drop); else the type table.
- **Measured, release, Apple M4 Max** (`crates/ffi/tests/measure_scene.rs`, `hot_calls_cost`; medians of 400): at 500 nodes and 800 wires, `plum_hit_test` 0.5 µs, `plum_wire_target` 0.33 µs, `plum_wire_paths` for 20 faces 5.9 µs; at 2,000 and 3,200, 1.6, 1.4 and 5.3 µs. A `plum_geometry_set` of every face reached the snapshot in 5.7 ms (500) and 14 ms (2,000).

### 9.6 A drag's moves (N2 slice 4)

`plum_gesture_move(doc, x, y, slop) → plum_drag`: where the drag under way in document `doc` is with the pointer at `(x, y)`: `kind` (1 move, 2 wire, 3 marquee, 4 bend point, 5 frame, 6 frame's size, 7 open group, 8 a number scrubbed: since N4 slice 2, its value now an `f64` whose low and high 32 bits are `rect[0]` and `rect[1]` bit-cast, as `plum_wire_paths` sends a nid, and `rect[2]` the same as an `f32`; the struct is unchanged, so `PLUM_ABI_VERSION` stays 1), `serial` (as the drag's `begin` replied), the offset `dx, dy` from where it began, a marquee's `rect`, and a wire's `target` (a `plum_hit`: the pin it would land on and `fits`, as `plum_wire_target`). Nothing changes. It reads the document's drag (a cell the core writes when `ui.gesture` begins or ends one, before its reply) and its snapshot, from any thread, without waiting for the core. `PLUM_ERR_MISSING` when no drag is under way or no scene is published, `PLUM_ERR_ARG` for a point that isn't a number. A shell moving faces calls `plum_wire_paths` with the `faces` the `begin` named and the `dx, dy` this gives. Measured: see plan section 17, slice 4.

## 10. Bytes

### 10.1 Borrowing

`plum_blob(handle) → plum_bytes {ptr, len, w, h, stride, format, status, token}`; `plum_release(token)`.

- `status`: 0 ready, 1 pending (a derived picture being made; the `pictures` domain says when), 2 missing (reaped: show "re-run to refresh"; the core asks for a rerun once, `docs/interface.md` 4.6), 3 refused (over budget after a memory warning), 4 unreadable (wrong shape).
- **Zero copy.** A store blob is often a mapped segment (`Bytes::Mapped(Arc<Segment>)`, `crates/store`): `ptr` points into it and the core holds the `Arc` until `plum_release`. An owned buffer is held the same way. On Apple it backs a `CGDataProvider`; on Android a direct `ByteBuffer`, copied once into a `Bitmap`.
- **Lifetime.** Valid until released, whatever happens to the document. Releasing twice, or an unknown token, does nothing. A shell SHOULD release once its texture is made.
- **Cache key.** A content handle names the same bytes forever (`docs/interface.md` 4.4), so it is the shell's texture-cache key. A derived ref (10.2) is too.

### 10.2 Picture formats

What the editor uses today, stated as the contract:

| `format` | Layout |
|---|---|
| 1 `rgba8` | 8-bit R, G, B, A; **straight (unpremultiplied) alpha**; sRGB; rows top to bottom; `stride` = `w·4`, packed. Engines' `rgba8` renditions are `(h, w, 4) \|u1` arrays (`run_view::rgba_image`, `crates/ui/src/run_view.rs:237`, uploaded with `from_rgba_unmultiplied`); decoded images (`image` crate) and SVG rasters (resvg, demultiplied, on white, `crates/svg`) are the same |
| 2 `bytes` | The value's own bytes (a PDF for PDFKit, an STL), with `media.describe` for type and shape |
| 3 `f32` | Little-endian f32, row-major, shape from `media.describe` (an array for a native grid or probe) |

Android's `Bitmap` is premultiplied: the Kotlin shell premultiplies in its one copy (open question 9).

**Derived refs** name pictures the core makes on workers (`plumcraft-media`): `derive:image:<handle>@<px>` (an encoded image decoded and shrunk), `derive:svg:<handle>@<w>x<h>`, `derive:pdf:<handle>#<page>@<px>`, `derive:spectrogram:<handle>`, `derive:compare:<a>:<b>:<how>`, `derive:mesh:<handle>@<camera>@<w>x<h>`. They are produced on demand by `plum_blob` (status pending) and stay valid as cache keys. Sizes are capped at 4096 a side (`media::MAX_TEXTURE_SIDE`) and 16 Mpx for a render (`MAX_RENDER_PIXELS`); decoding refuses past 16384 a side or 256 Mpx (`media.rs:41-46`).

### 10.3 Full values

A viewer that needs a value's numbers (the probe, compare, the values grid, playback) borrows its `full` handle as `bytes` or `f32`. The core shares one copy per handle (`Fulls`, weakly held). No number shown is ever made from an 8-bit picture (`docs/control-protocol.md`, `viewer.probe`).

### 10.4 `px`

A watch asks an engine for pictures at most `px` on the long side (`WatchRequest.px`). **Today** `px` is 1024 (`DEFAULT_PX`) unless `viewer.zoom` raised it, up to 4096 (`viewer_cmds.rs:26-29`, `:243-263`); nothing sets it from the view's size. **Proposed:** `viewer.size {popout?, px}` (a new command, not journaled) sets the panel's or a pop-out's `px` from the view's size in device pixels, capped by `max_picture_px`, and the zoom rules keep raising it for detail.

## 11. Asks

### 11.1 Shape and life

An ask is `{ask: u64, doc, kind, modal, params}` in the feed. The shell shows the platform's own UI and answers with `ask.answer {ask, answer}` or `{ask, cancel: true}`.

- **Modal asks** (`file.*`, `folder.pick`, `prompt`, `form`, `text`) are one at a time per document; while one is open the document's keyboard shortcuts wait (today, `lib.rs:1551`). Others queue in order.
- **Non-modal asks** (`clipboard.*`, `share`, `open.external`, `screenshot`) may overlap.
- **No timeout** for asks a person answers. `screenshot` times out after 2 s and the core replies to `ui.screenshot` as today's fallback rules say.
- **Withdrawal:** the core withdraws an ask whose reason went (its document closed, its node was removed); the shell closes its UI.
- **Scripts:** a control client sees pending asks in `ui.inspect.asks` and answers with `ask.answer` (or `ui.prompt` for a `prompt`). The first answer wins; the shell then closes its UI. Headless (no shell), only scripts answer.

### 11.2 Kinds

| Kind | Params | Answer |
|---|---|---|
| `file.open` | `{purpose, title, kinds: [{label, extensions}], multiple, startIn, prompt?}` | `{paths: [..], grants?: [..]}` |
| `file.save` | `{purpose, title, kinds, name, startIn}` | `{path, grant?, commit?: bool}`; `commit: true` when `path` is in scratch and the shell will stream it (11.3) |
| `folder.pick` | `{purpose, title, startIn}` | `{path, grant?}` |
| (purpose `path`, B133) | `file.open {purpose: "path", title, kinds: [{label, extensions}], multiple, name?, hint: {mode, filters}, current, startIn}` for a file the node reads (`multiple` for `files`), `file.save` for one it writes (`hint.mode` `save`), `folder.pick` for a folder (`dir`) | As the kind's answer above: a path an engine can open. A shell that can't hand back a lasting path (iPadOS's document picker gives a security-scoped URL) answers with a copy it made in a folder of the app's (the Apple shells: below, 11.4) |
| `prompt` | `{kind, title, text, answers, labels, detail?}`; `labels` are the buttons' words by answer (`saveShape`: "Save as PNG anyway", "Save as NumPy (.npy)", "Cancel"): a shell shows them when given and falls back to its platform's words for an answer (the Apple apps: `AskWords.buttonTitle(_:prompt:)`) | `{answer}`, one of `answers` |
| `form` | `{command, title, intro, fields: [{key, label, hint, kind: "line" \| "secret" \| "text" \| "pick", required, choices?, other?}], submit, values}` | `{values: {key: text}}`; the core runs the command (`CommandForm::params`, `commands.rs:201`) and, on an error, raises the form again with `error` |
| `text` | `{purpose, title, label, text, placeholder, hint?, validate?}` | `{text}` |
| `clipboard.set` | `{text, kind: "workflow" \| "text"}` | `{}` |
| `clipboard.get` | `{kinds}` | `{text}` or cancel |
| `share` | `{path, subject, target?}` | `{notice}` ("Copied", "Sent") or `{error}` |
| `open.external` | `{path?, url?, app?}` | `{}` or `{error}` |
| `screenshot` | `{path, offscreen}` | `{path, width, height, source}` |

**The pick-list field (decision 5, N4 person gate: "Add the pick-list field kind").** A field of `kind: "pick"` carries `choices: [{value, label, hint}]` (in order; `hint` may be empty) and `other: bool`. A shell draws it in the form's grid like any field, its label beside it: a pop-up button or combo box listing each choice's `label` (its `hint` as a tooltip or subtitle), and, when `other` is true, a one-line text field for any value not on the list (the field's own `hint` as its placeholder; a combo box whose text is editable is the same thing). The answer is unchanged: the field's text is the chosen choice's `value`, or what was typed. Without `other`, the core refuses any text that isn't a choice's `value` (the answer, as a wrong shape, leaving the ask open; and `CommandForm::params` again, naming the field). The form opens with `values[key]` as the current text: a shell shows the choice whose `value` it is, else the text as typed. Install Engine… (`engine.install`) is the one pick field today: `repo`, `other: true`, the public engines `plumcraft-<name>-engine` under the repository owner's GitHub account as choices (`install::KNOWN_ENGINES`, the one list; the egui editor's Install window draws the same field from `invoke::engine_form`). Conformance: "Install Engine…'s form offers the public engines as a pick list, with any other repository typed" (`needs: ["install"]`: a driver whose platform can install and whose core asks with the form).

**As built (N3 slice 5), the iPad's answers.** `file.open` and `folder.pick`: the document picker, open in place, the security-scoped access kept for the document's life. `file.save`: an alert naming the file, written into the document's own folder when the app can write there, else its Documents folder (the commit flow of 11.3 for a destination without a path is not in the core). `prompt`, `text`, `form`: alerts. `clipboard.*`: `UIPasteboard`. `open.external`: a URL opens in its app; a file goes to the share sheet. `share`: the share sheet with the file, answered `{notice: "Shared"}` or cancelled; no command raises it yet, since no shell declares `shell.share_targets`. `screenshot`: the canvas's own picture. As on the Mac, an ask a script started is left for the script.

**The egui editor's answers (after N4).** It has no share sheet on any platform it runs on (the Mac's `NSSharingServicePicker` would need `unsafe` outside `crates/ffi`). For a day it answered the person's `share` by showing the file in the file manager; **since 2026-11-21 (the maintainer: "Disable") it declares no share sheet** (`Document::share = false`, 4.2's `share`), so the core disables File › Share…, the toolbar's Share and the pop-out bar's Share… with "Sharing isn't available in this editor", refuses `value.share` and `graph.share` with it from a script, and raises no `share` ask; egui no longer answers one. The pop-out's bar (Copy, Save As…, Share…, Pin as A, Pin as B) is the core's commands for that pop-out: `value.copy`, `value.saveAs` and `value.share` invoked for the person (the core raises `clipboard.set`, `file.save` and `share`), `viewer.pin {slot, popout}` run. **No drag out in egui:** neither egui nor winit can start a drag of a file out of a window, so `value.export` stays a command there.

`prompt.kind` and `detail`: `unsavedChanges` (`answers: [save, discard, cancel]`, `then: new | open | close | quit`); `reviewWorkflow` (`[trust, cancel]`, `detail`: the `trust::Review` of what the graph touches); `allowUploads` (`[trust, cancel]`, `detail: [{to, file}]`). These are today's (`crates/ui/src/lib.rs:321-373`), with today's texts.

`text.purpose`: `rename.node` (`node.title`), `rename.frame` (`frame.set {title}`), `rename.group` (`group.rename`), `rename.net` (`net.rename`), `name.port` (`graph.port.name`), `rename.port` (`graph.port.rename`), `label.uses` (Label All Uses…, `wire.label` on every wire from the output), `slice` (`wire.slice`, hint "start:stop:step or an index"). Today these are windows of the editor's own (`rename_view.rs`, `net_view::NameEdit`, `slice_view::SliceEdit`).

### 11.3 The file rule

- The core and engines only see ordinary paths inside the declared roots or returned by a file ask (plan section 6).
- **Grants** are opaque strings the shell keeps (an iOS security-scoped bookmark, an Android persisted URI permission). The core stores `{path, grant}` in recent files and in a document's record, and hands the grant back in `document.open` so the shell can reopen access first.
- **A save to a destination without a path** (Android SAF, an iOS export): the shell answers `file.save` with a path in `dirs.scratch` and `commit: true`; the core writes there, sends `committing {ask, path}`, the shell streams it to the destination and calls `file.commit {ask, ok, error?}`. The save is in progress until then.
- **An open without a path** (a content URI, a photo): the shell copies the file into `dirs.caches/imports/<digest>.<ext>`, named by its content so a second pick costs nothing, and answers with that path.
- **Photos as a source for a path (feat/path-photos; decided 2026-11-23, 11A: the shell's business only).** When a `file.open` with purpose `path` takes pictures only (every extension in `kinds` a picture's: png, jpg, jpeg, heic, heif, gif, tif, tiff, webp, bmp; `PickedFiles.picturesOnly`), the iPad and iPhone ask the person "Photos or Files" (an action sheet, `path-source`), then show that picker. A photo is kept as it is when its form is one of the kinds, else converted to the first of PNG or JPEG the kinds name, else the ask is cancelled with a message. Either way the copy is kept in Documents › Picked Files (`PickedFiles.keep`, named by the library's suggested name or "Photo <date> at <time>", `PickedFiles.photoName`) and the answer is `{paths}`, as for Files. **What the core sees:** one ask and one answer, a path; nothing says which source gave it. **How a script drives it:** a script that answers asks answers this one with a path, as any `file.open`; the choice is part of answering the ask, as the document picker's own sidebar is, so it adds no command and no field. The Mac's open panel already lists Photos in its sidebar. **Declined (11B):** the core saying in the ask that Photos is offered (`sources`) and an answer saying which source gave it (`source`); the contract is unchanged. The test engine's `ports.picture` node (a path that takes PNG and JPEG only) launches it in the iPad's UI tests (12A).
- `startIn` is the folder this purpose last used (`prefs.dirs`, keys in `prefs::keys`, `crates/ui/src/prefs.rs:31`), else the document's folder, else `dirs.documents`. The core remembers the folder of every answer, as `ask_dialog` does (`lib.rs:658`).

### 11.4 Today's dialogs and prompts, one for one

| Today | Ask |
|---|---|
| `FileDialogs::pick_path(hint, current)`, mode `open` or empty | `file.open {purpose: "path", kinds: hint.filters, startIn: the field's folder, else prefs.dirs[dir_key]}`; answer → `paths::chosen_path` (relative to the workflow's folder) → `node.set` (`lib.rs:2182-2207`) |
| `pick_path`, mode `save` | `file.save {purpose: "path", name: the field's file name}` |
| `pick_path`, mode `dir` or `folder` | `folder.pick {purpose: "path"}` |
| `pick_files(hint, current)` | `file.open {purpose: "files", multiple: true}`; answer → one path per line → `node.set` |
| `start_in(dir)` | `startIn` on every file ask (11.3) |
| `open_workflow()` | `file.open {purpose: "workflow", kinds: [{label: "Workflow or bundle, <openers>", extensions: ["plum", "json", "zip", …openers]}]}` (`lib.rs:231`) for `graph.open`, `graph.insert`, `graph.insertGroup` |
| `save_workflow(current)` | `file.save {purpose: "workflow", kinds: [{Graph, [plum]}, {Graph as JSON, [json]}], name: current's stem or "untitled", with ".plum"}` for Save of an unsaved graph and Save As (12.23) |
| `save_bundle(current)` | `file.save {purpose: "bundle", kinds: [{Bundle, [zip]}], name: "<stem>.zip"}` (`graph.bundle`) |
| `pick_app()` | `file.open {purpose: "app", kinds: [{Application, [app]}], startIn: "/Applications"}` on macOS; `value.open`'s Choose an App… is disabled without `subprocess` |
| `save_output(formats, suggested)` | `file.save {purpose: "result", kinds: catalogue.save_formats, name: save_cmds::suggested_name, startIn: the last save's folder}` (`lib.rs:876`) |
| `open_theme()` / `save_theme(name)` | `file.open` / `file.save {purpose: "theme", kinds: [{PlumCraft theme, [json]}], name: "<theme>.plumtheme.json"}` (Settings › Import…, Export…) |
| Spike `waiting()`, `take_answer()` | The ask mechanism itself |
| Spike `SendTo::targets()`, `send()`, `take_notice()` | `shell.share_targets` in the config; `share`; its answer's `notice` goes to the status messages |
| `Prompt::Discard {New, Open, Quit}` | `prompt` `unsavedChanges`, `then` `new`, `open`, `quit`, and `close` for one window |
| `Prompt::Review` | `prompt` `reviewWorkflow` |
| `Prompt::Uploads` | `prompt` `allowUploads`; `trust` allows the files and resends the held run (`lib.rs:1114`) |
| `CommandForm` (`form_view.rs`), Engine › Attach… (`AttachForm {url, token}`), Install Engine… (`InstallForm {repo}`) | `form`; Attach and Install become `CommandForm`s of `engine.attach` and `engine.install` |
| Rename…, Name This Port…, Rename Port…, Rename Net…, Label All Uses…, Slice… | `text` (11.2) |
| `ctx.copy_text` for Copy, Cut and `viewer.recipe {copy: true}` (`lib.rs:1996`, `edit_view.rs:469`) | `clipboard.set` |
| Paste: `ViewportCommand::RequestPaste`, then egui's `Paste` event (`edit_view.rs:503`) | `clipboard.get`, then `edit.paste {text, pos}`. A shell that reads the pasteboard itself sends `edit.paste` directly |
| `doc.open`, `value.open` start a program from the session (`open_cmds`, `/usr/bin/open`, `xdg-open`) | `open.external {path, app?}` where `subprocess` is false; unchanged on desktops |
| `ui.screenshot`, `--screenshot` | `screenshot` |

What `EditorApp::invoke` fills in (`lib.rs:961-1072`) becomes the core's invoke state machine, the same per id: a form first; `graph.new`/`graph.open` through `unsavedChanges` then (for open) `file.open`; `app.quit` through the prompt; `graph.save` with a path runs, without one `file.save`; `graph.saveAs`, `graph.bundle`, `graph.insert`, `graph.insertGroup` through their file asks (insert at the canvas centre, then select the result); `group.create` with the selection's nodes and groups; `node.expand` with the one selected node; `group.ungroup` with `group_to_ungroup` (`lib.rs:1076`); `frame.add` with the selection's sizes; `graph.tidy` with the selection (two or more) and measured sizes and pins, then fit; `node.remove`, `node.duplicate` with the selection (duplicates become the selection); an application's command with `{nodes, group}` and its `notice` to the status bar. "Select nodes first" and similar stay status messages, not asks.

## 12. The menu and command model

### 12.1 `menu.get`

```json
{"gen": 31, "menus": [
  {"title": "File", "items": [
    {"command": "graph.new", "label": "New", "shortcut": {"mods": ["primary"], "key": "N"}, "enabled": true},
    {"command": "graph.save", "label": "Save", "shortcut": {"mods": ["primary"], "key": "S"}, "enabled": false, "reason": "nothing to save"},
    {"separator": true},
    {"title": "Export", "items": [ … ]}]},
  {"title": "View", "items": [{"command": "view.runColors", "label": "Colour Nodes While Running", "checked": true, "enabled": true},
                              {"title": "Animation Speed", "items": [{"command": "view.animationSpeed", "params": {"ms": 0}, "label": "Real Time", "checked": false, …}]}]}]}
```

- Built from the registry (`command_specs`, `commands.rs:273`): top-level menus in the order File, Edit, View, Node, Run, then any a command names (Group, Engine, Help today), each command at its `menu` path, submenus for longer paths (`lib.rs:2251-2290`), in registry order.
- An item: `command`, `params?` (an item that runs a command with fixed parameters), `label`, `shortcut?`, `enabled`, `reason?` (the disabled item's tooltip), `checked?` (today `run.auto` and `view.runColors`, `lib.rs:2242`), `hover?`, `warning?`, `form?: true` (it asks first: the shell adds `…` per platform style).
- **`warning` (2026-11-21, additive):** something wrong with what the item stands for that doesn't disable it. Today only the Engine menu's items for an engine whose manifest warned (`docs/interface.md` 16.6: very likely a copy of a project's manifest, which won't start): the engine's submenu, each of its hostings, and its one item when engines are picked automatically. The item stays enabled and its `hover` starts with the same text, so a shell that shows only `hover` still says it. A shell marks the item (the egui editor puts ⚠ before the label, in the theme's `warning` colour) and shows `warning` on hover. `engine.list`'s `choices` and `ui.inspect`'s `engine.choices` carry the same `warning` on each choice of that engine. What the native shells do with it today, unchanged: the Mac app's menu items take `hover` as their tooltip when enabled, so the text shows on hover but there is no mark; the Windows app's items take `hover` as their tooltip, the same; the iPad's menus show only a disabled item's `reason` (as the subtitle), so neither shows. To mark it, each decodes `warning` (its `MenuItem` models ignore unknown keys today) and draws: the Mac, a ⚠ before the title (or `NSMenuItem.image` with the `exclamationmark.triangle` symbol) and `warning` as the tooltip; the iPad, the same symbol as the `UIAction`'s image and `warning` as its `subtitle`; Windows, a `FontIcon` warning glyph as the `MenuFlyoutItem`'s icon (or ⚠ before the text) and `warning` as its tooltip.
- Items that are not plain registry entries come from the core too (12.3): the Engine menu's choices, Node › Add by category and by engine, Animation Speed, the Edit menu's canvas items (commands since N2 slice 1, Align › and Distribute › excepted), View › Fit, Node › Search…
- Activating an item is `ui.invoke {command}` (or `command.run {command, params}` when it has `params`), never a shell-side interpretation.
- **As built (N3 slice 4): `ui.invoke {command, by: "person"}`.** Over the C ABI a shell's own request may say who it invokes for: `by: "person"` (its menu bar, a context menu, a toolbar button) raises what the command asks as the person's (`by: "person"` in the ask), for the shell to show natively; `by: "script"` or no `by` is a script's, as before, which a shell leaves for the script to answer. Only `plum_request` takes the field: the dispatcher every control channel uses refuses it as an unknown field, so a script can't make a shell put a panel on the screen.

### 12.2 Shortcuts

Today's notation is egui's: `Cmd+Shift+Z` (`CommandSpec::shortcut`, `commands.rs:19`; parsed by `parse_shortcut`, `lib.rs:2316`). The menu model gives `{mods, key}`:

| Token today | `mods` | macOS | iPadOS | Android, Linux, Windows |
|---|---|---|---|---|
| `Cmd` | `primary` | ⌘ | ⌘ (`UIKeyModifierFlags.command`) | Ctrl |
| `Ctrl` | `ctrl` | ⌃ | ⌃ | Ctrl |
| `Shift` | `shift` | ⇧ | ⇧ | Shift |
| `Alt`, `Opt` | `alt` | ⌥ | ⌥ | Alt |

`key` is egui's key name: letters, digits, `Enter`, `Period`, `Comma`, `Escape`, `Tab`, `Space`, `Delete`, `Backspace`, arrows. The registry's shortcuts today: `edit.undo` Cmd+Z, `edit.redo` Cmd+Shift+Z, `graph.new` Cmd+N, `graph.open` Cmd+O, `graph.save` Cmd+S, `graph.saveAs` Cmd+Shift+S, `app.quit` Cmd+Q, `app.settings` Cmd+Comma, `node.duplicate` Cmd+D, `group.create` Cmd+G, `group.ungroup` Cmd+Shift+G, `graph.tidy` Cmd+Shift+L, `run.start` Cmd+Enter, `run.cancel` Cmd+Period. Rules kept from today: the most specific shortcut matches first (`lib.rs:1559`); a disabled command still consumes its keys; `edit.*` shortcuts are left to a focused text field; a shortcut fired in a text field runs after the field commits (`lib.rs:1575`).

**As built on the Mac (N3 slice 4).** The Mac app builds its menu bar from `menu.get` and the feed's `menus` changes, and maps a shortcut to a key equivalent only when it has a modifier besides Shift: a plain key (Backspace for Delete, F for Fit, Escape) would take typing from a text field, so the canvas recognises those itself (12.3). Items with a role are the system's standard actions (NSDocument's New, Open, Save, Save As; Undo, Redo, Cut, Copy, Paste, Delete, Select All down the responder chain, so an editing text field takes them), About, Settings… and Quit go in the application menu, and Undo and Redo show the step names from the `document` domain (7.1).

### 12.3 What was not a command, and is since N2 slice 1

Rule 3 was not met in these places; each is now a core command (`crates/editor-core/src/canvas_cmds.rs`), not journaled (Delete's `node.remove` is), listed by `command.list` after the registry's (open question 2, answered A; the capability `commands.canvas` can't be listed yet, since nothing serves `core.hello`'s `caps`), and named by the menu model. The selection (`{nodes, groups, net}`, `crates/editor-core/src/selection.rs`) and the group entered (`Document::focus`) are the document's. Fit, the palette and reveal are the view's: the core checks them and hands them to the shell through `Holder::shell_command`, the hook a shell-side command already took; a shell with no canvas (the C ABI's core today) answers "isn't available from this shell". `docs/control-protocol.md` lists each command's params and reply.

| Before | Where it was | Command now | Where it runs |
|---|---|---|---|
| Click, Shift-, ⌘-click, marquee | `edit_view.rs:140` | `selection.set {nodes?, groups?, mode: replace \| add \| remove \| toggle}`; a click on empty canvas `selection.clear` (the net stays) | core |
| Edit › Select All, ⌘A | `edit_view.rs:339`, `:576` | `selection.all` | core (what is shown: `selection::Visibility`, moved from `groups_view.rs`) |
| Edit › Cut, Copy, Paste, ⌘X/C/V | `edit_view.rs:389-596` | `edit.cut`, `edit.copy`, `edit.paste` with menu places and shortcuts; `ui.invoke` fills in the selection (with selected collapsed groups' members) and raises `clipboard.set` or `clipboard.get` | core (`invoke`) |
| Edit › Delete, Delete, Backspace | `edit_view.rs:466` | `edit.delete`, not `node.remove` itself: it takes the members of selected collapsed groups, which the Node menu's Remove doesn't, and clears the selection | core |
| Edit › Frame Selection | `edit_view.rs:501` | `frame.add`, invoked: since N2 slice 3 the core fills in the selection and the sizes it was told (`Host::frame_selection`'s default, `Host::sizes`) | core |
| Edit › Align ›, Distribute › | `edit_view.rs:473-480` | Since N2 slice 3, `selection.align {edge}` and `selection.distribute {axis}`: the selection at the sizes the shell reported, run as `edit.align` and `edit.distribute`; the menu model's items carry them | core |
| View › Fit, F | `lib.rs:1629`, `canvas.rs:586` | `canvas.fit` (`ui.set {fit: true}` runs it); since N2 slice 3 the core works out the rect (8.9) and the shell fits its view to it | core rect, shell view |
| Node › Search…, Tab, Space, right-click on canvas | `lib.rs:1644`, `canvas.rs:585`, `:873` | `canvas.palette {open?, at?, query?, wire?}`, `at` in graph coordinates (`ui.set {palette}` runs it); a pick is `palette.choose {kind}` | core state since N3 slice 1 (open question 4; 12.7); the shell draws it |
| Node › Add › (by category, by engine) | `palette::node_menus` | `node.add {kind, pos}` items in the menu model (since N1 slice 8) | core |
| Enter group, Leave, breadcrumb | `groups_view.rs:747-770` | `canvas.focus {group \| null}` (`ui.set {focus}` runs it) | core state; the shell rebuilds and fits |
| Reveal a node, a net tag's double-click | `canvas.rs:423`, `net_view.rs:324` | `canvas.reveal {node}` | shell |
| Esc cancels a run | `edit_view.rs:577` | `run.cancel` (already a command), bound to Escape by the shell while a run is in flight | core |
| Select Net, Go to Driver, next use | `net_view.rs:474-490` | `net.select {name?, group?}` (a tag's click toggles it, Esc and an empty click clear it); Go to Driver and Next Use stay shell items that resolve to `canvas.reveal` (next use cycles per tag, shell state) | core; reveal shell |

The keys (⌘A, Delete, Backspace, Escape, F, Tab, Space, ⌘X/C/V, and since N3 slice 1 the arrows: `selection.nudge {by: [±1, 0] | [0, ±1]}`, ten times that with Shift, 12.6) are recognised by the shell and dispatched as these commands; they are not in `shortcut_table()` (12.2), which stays the registry's, because a shell's text fields and popups decide when the canvas has the keyboard. The menu model shows them on its items.

### 12.4 Context menus

`menu.get {context: {kind, …}}` returns a menu like 12.1's. **As built (N1 slice 8):** the reply is `{context: <kind>, items: [item]}`, the items as 12.1's, not `{gen, menus}`; it has no `gen` and no feed change. The kinds and their items today:

| Kind | Items (command ids) | Source |
|---|---|---|
| `node` | Edit Parameters… (`node.parameters`); the application editor (`node.editor`); the id and kind (a label); Rename… (`text` `rename.node`); View Full, No preview, Collapsed (`node.view {nodes, view}`, the selection when the node is in it); Keep result When slow, Always, Never (`node.cache`); Run on <engine>, Any (`node.engine`); Watch <port> (`run.watch`); Save Result…, Contact Sheet… (`run.save {ask}`); Detach (`bundle.detach`); Expand into Group, Expand without a Group (`node.expand`); Open <port> in <App> (`doc.open`); Open <port>, Open <port> with › recent apps, Choose an App… (`value.open`); Duplicate (`node.duplicate`); Remove (`node.remove`); Group <selection>, Collapse into a group (`group.create`); Promote ›, Expose ›, export mark (`group.promote`, `group.unpromote`, `group.output`, `group.removeOutput`, `group.checkpoint`) | `canvas.rs:1472-1626`, `groups_view.rs:357-466` |
| `proxy` | Expand (`group.collapse`), Remove output (`group.removeOutput`), Watch <output> (`run.watch`), Rename… (`text` `rename.group`), Ungroup (`group.ungroup`) | `groups_view.rs:640-668` |
| `input` | Name This Port… (`text` `name.port`); or Rename Port… (`text` `rename.port`), Remove Name (`graph.port.unname`); a wired input says it can't be named | `canvas.rs:1062` |
| `output` | Watch <port> (`run.watch`) | `canvas.rs:1257` |
| `wire` | (B164: after a run, a note line when the wire reads a list joined from several runs: its index slice takes one run's result, or its input takes the joined list whole; the same words end the input row's `hover`) Watch (`run.watch`), Pop Out (`view.popout`), Slice… or Edit Slice… (`text` `slice`), Clear Slice (`wire.slice {slice: null}`), Add Bend Point (`wire.route {via}` with the point), Make Right-Angled (`wire.square {to}`, B155: a wire whose bend points don't line up, or one with none running right to left), Route Automatically (`wire.route {via: []}`; was Straighten), Straighten (`wire.route {via: []}`), Label All Uses… (`text` `label.uses`), Disconnect (`wire.disconnect`) | `canvas.rs:821-862` |
| `net` | Go to Driver or next use (`canvas.reveal`), Watch, Pop Out, Select Net (`net.select`), Rename Net… (`text` `rename.net`), Draw as Wire, Draw All as Wires (`wire.unlabel`) | `net_view.rs:470-510` |
| `frame` | Rename… (`text` `rename.frame`), Colour › swatches and Theme accent (`frame.set {color}`), Remove Frame (`frame.remove`) | `frame_view.rs:167-192` |
| `group` | Collapse (`group.collapse`), Enter (`canvas.focus`), Rename… (`text` `rename.group`), Ungroup (`group.ungroup`), the group's parameters (`group.editPromoted`, `group.unpromote` as a `form`) | `groups_view.rs:440-640` |
| `canvas` | Today the palette, not a menu. Proposed for touch: Add Node… (palette), Paste, Select All, Fit | `canvas.rs:873` |

A context menu's `params` name what was hit, so a shell passes the hit through: `menu.get {context: {kind: "wire", to: {node, port}, at: [x, y]}}`.

### 12.5 Forms and windows

`CommandForm`s are asks (11.2). The document's other windows are state in the `windows` domain (as built since N4 slice 3, the parameters inspector and the pop-outs in the scene's `view.windows`, 12.14), each opened and closed by its command: the parameters sheet (`node.parameters`), value editors (`node.editor`), Settings (`app.settings {tab}`), About (`app.about`), Messages (`view.messages`), the Cache Report (`run.cacheReport`), the graph's Parameters (`view.params`), pop-outs (`view.popout`, `view.close`). The shell draws each natively from the domain and the core's data for it; their controls are commands.

**As built for FR4 (research note 29):** the scene's `view.windows` also carries `acknowledgements` while the Acknowledgements window is open (`app.acknowledgements {open?}`, Help › Acknowledgements, beside About in the menu model, and a button in every shell's About). Its model is `acknowledgements.get {shell?, query?}`: `{open, editor: {name, list}, engines: [{id, name, given, list?, error?}], query}`, each `list` `{rows: [{licence, name, packages: [{name, version, url, notices}], texts: [{text, packages, standard}]}], packages, shownPackages, note}`: one row per licence id, as the maintainer asked ("coalesce license variations"). Texts the same once their copyright lines (and Apache-2.0's appendix), whitespace, line wrapping and a title are set aside are one `standard` text, shown without its copyright lines; each package keeps its own copyright lines (and what its Apache-2.0 appendix adds) as `notices`, none dropped; a text that differs in substance (a changed licence, an added exception) is its own entry, `standard: false`, with the packages whose it is. An engine's list is coalesced the same way here, its data on the wire unchanged. `shell` (`mac`, `ios`, `windows`, `egui`) leaves out what another shell alone ships (the Windows App SDK's terms, the egui font); `query` keeps the licences whose id or name holds it, whole, and the packages whose name holds it, in their licence's row, so every shell's search is the core's. An engine with `given: false` shows "no list given" (or its `error`). A shell draws it as a scrolling, sectioned list with selectable text: a window on the Mac, Windows and egui, a sheet from About on the iPad and iPhone; closing it runs `app.acknowledgements {open: false}`.

**As built in N4 slice 6:** the scene's `view.windows` also carries `cache` (the Cache Report is open), `params` (the graph's Parameters window), `editors` (the nodes whose value editor is open), `compare` (Compare is open) and `reports`, a stamp that changes when what an open one reads changes. The core draws the first three as a model, `report.get {window, node?}` (sections of rows: label, value, fields a command takes, buttons that are commands), and Compare's sides as `compare.get`; the Apple apps draw them in the platform's grouped list (a window each on the Mac, a sheet on the iPad and iPhone) and close them with the commands that opened them. A value editor draws the port's value as JSON whatever its kind (an application's own editor for the kind is the egui editor's). Engine › Attach… and Install Engine… raise the core's `form` ask (11.2) in a shell with no window of its own for them (`Host::open_window` false), and run the command with what was typed; Install Engine… stays disabled where the core can't install (the C ABI's; since B71 the core installs wherever the platform declares `subprocess` and `python`, the Mac app included, and is disabled with the reason elsewhere, section 4.1's B71 paragraph). Nothing the Apple apps' menus offer is left on "isn't available yet".

### 12.6 Moves are undo steps (N3 slice 1)

The maintainer's N2 exit decision 3: "a move drag becomes one undo step, in the core, for every shell". The session's `Session::edit_move` makes it so for every command that moves:

| Command | Label | Joins the step before when |
|---|---|---|
| `edit.move {nodes?: {id: [x, y]}, groups?: {id: [x, y]}, drag?}` (new; what a face drag and a nudge send) | "Move Node", "Move 3 Nodes", "Move Group", "Move 2 Groups", "Move 4 Items" | the same nodes and groups |
| `node.move {id, pos, drag?}` | "Move Node" | the same node |
| `group.move {group, pos, drag?}` | "Move Group" | the same group |
| `frame.move {frame, pos, nodes?, groups?, drag?}` | "Move Frame" | the same frame |
| `frame.resize {frame, size, drag?}` | "Resize Frame" | the same frame's size |
| `selection.nudge {by: [dx, dy], drag?}` (new, a canvas command: the selection's `edit.move`) | as `edit.move` | as `edit.move` |

- **The rule.** A move joins the undo step on top of the stack when that step was made by a move of exactly the same things, nothing has made, undone or redone a step since, and either both name the same `drag` (a string of 1 to 64 characters or a whole number) or neither names one and the last move that joined came at most one second ago (`MOVE_COALESCE`). A move naming a drag never joins a step that named another, or none. The step keeps the label it was made with. A move to where things already are changes nothing: no step, and the document doesn't become dirty.
- **What a shell names.** A shell that sends moves while dragging (the egui shell, a frame each) names the press, `drag: "egui:<press>"`, so the drag is one step however long it takes; `ui.gesture` names its own (`"gesture:<serial>"`). For the arrow keys a shell SHOULD name each key press and let its repeats join it (`drag: "key:<press>"`; on the Mac, `NSEvent.isARepeat`; egui's `Event::Key { repeat }`), so a held key is one step and two taps are two; a shell that can't tell sends no `drag`, and the one-second rule holds a held key together.
- **Moves are still not journaled** (they were not; a drag would fill the journal), and make nothing stale. Tidy ("Tidy"), Align ("Align") and Distribute ("Distribute") were undo steps already and are unchanged; a move after one of them is a step of its own.
- **Not undo steps, as before:** views (`node.view`, `node.fields`), net and wire labels' places (`net.move`, `wire.labelAt`), and everything that isn't an edit.

### 12.7 The palette, and a wire dropped on empty canvas (N3 slice 1)

Open question 4 (decided A) made the palette core state; question 8 (decided 8A, the maintainer's N2 exit decision 8) adds a new wire dropped on empty canvas.

- **`canvas.palette {open?, at?, query?, wire?}`** opens (or, `open: false`, closes) the palette, kept by the core and shown in the scene as `view.palette {at, query?, wire?}`. `wire` is `{from: {node, port}}` (the output a new wire leaves) or `{to: {node, port}}` (the input it enters), checked against the graph. The reply is `{open, at, query, wire?, offered?}`, `offered` counting the kinds that take the wire. No undo step; closing leaves nothing behind. A core without a canvas no longer refuses it.
- **`palette.items {query?, limit?}`** (a method, not a command) → `{items: [{kind, label, category, port?}], total, query, wire?}`: what the open palette offers, matching every word of `query` (default the palette's own): a kind whose title is the query itself (case and runs of spaces aside) first, then the rest, each in category order (decision 13 after the person check, 2026-11-21: the ranking is the core's, for every shell), at most `limit` (default and most 150); with a wire only the kinds with a port that takes it, each with that `port`. The fit is `wire.connect`'s (types, list-ness, wireability), checked for a node that doesn't exist yet: the first input (from an output) or output (into an input) in the kind's definition that would connect. Core's Convert node is not offered for a dropped wire, and a wire from or into a Convert offers nothing (its rules are its own).
- **`palette.choose {kind, port?, at?}`** (a canvas command) adds a node of `kind` where the palette opened (or at `at`), and when the palette has a wire connects it, as **one undo step** through `node.addWired` ("Add and Connect"); without a wire it is `node.add` ("Add Node"). The palette closes. A kind that doesn't take the wire is refused with the reason, and the palette stays.
- **`node.addWired {kind, pos?, from | to, port?}`** (a session command, journaled): the node and the wire in one step; `port` defaults to the first that fits. Contract question 8 named it.
- **Kinds whose engine isn't started (N4 slice 1).** With engines picked automatically (4.1, decision 8C), `palette.items` also lists the kinds of engines the core knows without starting them (the engines compiled in) whose engine this document hasn't started, each with `starts: <engine>`, among the rest in category order and filtered for a dropped wire alike. `palette.choose` of one is held as `node.add`'s is: the engine is asked for and the request (and every request after it for that document) waits until it is up, then adds; the reply comes then. A shell shows the pick as pending ("Starting the array engine…") and keeps the palette open until the reply, rather than closing it at once. With picking off nothing is listed that would be refused. **The egui editor (after N4)** fills `Engines::offered_defs` from its engine choices as the C ABI core does (`plumcraft_editor_core::engines::offered_defs`) and draws the same list (`startable_defs`, through `palette::with_starts`), each such kind labelled "· starts the array engine"; a pick closes its palette at once and the add is held by the document (4.1 above), the status bar showing the engine connecting.
- **Testing kinds are not offered (the maintainer, 2026-11-21, card 31 B: "hide Testing kinds from the palette").** A kind with `Testing` anywhere in its category path (the text engines' Every widget, `text.widgets`, and Slow, `text.slow`) is never listed by `palette.items` (for any query, a dropped wire or an engine not started) nor in the Node › Add menu (`menu.get`), in every shell, the egui editor's palette included (`plumcraft_editor_core::palette::offered`). `node.add {kind}` by id still adds it: the conformance cases and tests use them so.
- **Who sends what.** `ui.gesture`'s drag of a new wire ending on nothing runs `canvas.palette` with the wire (8.13), on every shell. A shell draws the palette from `view.palette` and `palette.items`, and sends `palette.choose` for a pick and `canvas.palette {open: false}` for Escape or a click away. The egui shell does all of it, and originates the drop from egui-snarl's `has_dropped_wire_menu` (8.13).

### 12.8 Run Selected (N4 slice 1)

`run.selected {}` is a canvas command (golden rule 4; the maintainer: "Do it. No exceptions to the rule."): `run.start {targets}` with the selected nodes and the members of selected collapsed groups. It is Run › Run Selected in the menu model, after Run, disabled with the reason ("select nodes to run what they need", or `run.start`'s); `command.list` lists it with `run.start`'s rule. Before, the Swift apps sent `run.start {targets}` themselves.

### 12.9 The Apple shells' editing gestures (N4 slice 1)

What each Apple shell recognises and sends; the meaning is 8.13's, the core's.

| Meaning | macOS | iPadOS |
|---|---|---|
| Move faces, a frame, a group, a bend point | Drag | One-finger drag on it (a pointer's or Pencil's too) |
| Wire, pick up a wire's end | Drag from a pin | One-finger drag from a pin, 22-point slop (44 across); a haptic when the target pin fits |
| Marquee (plain replaces, Shift adds, ⌘ takes away) | Drag on empty canvas, a wire, a tag or a frame's body | The same with the Pencil or a pointer, or with one finger in Select mode (a toolbar toggle); otherwise one finger there pans |
| Cancel a drag | Escape | Escape, a second finger, the system |
| Autoscroll | Within 36 points of the view's edge while dragging | The same |
| Palette | Double click or secondary click on empty canvas, Tab, Node › Search…, the toolbar's Add Node | Double tap or long press on empty canvas, Tab, Node › Search…, the toolbar's + |
| Context menu | Secondary click, Control-click | Long press, secondary click |
| Undo, Redo | ⌘Z, ⇧⌘Z | Two-finger tap, three-finger tap; ⌘Z, ⇧⌘Z (the system's three-finger editing gestures are off on the graph) |
| Tidy | ⇧⌘L | Two-finger double tap; ⇧⌘L |
| Fit | The toolbar's Fit, F, View › Fit; smart zoom fits one node | The toolbar's Fit, F, View › Fit |

The drag itself is the kit's `CanvasDrag` (`PlumCraftCanvas`) on both: `ui.gesture` drag `begin`, `plum_gesture_move` each frame (faces moved and their wires re-pathed from `plum_wire_paths`; a wire's target ringed when it fits, dashed through a Convert, crossed when it doesn't), `end` or `cancel`. A new wire dropped on nothing stays drawn while the palette it opened is open. The palette is the kit's `PaletteModel` over `view.palette`, `palette.items`, `palette.choose` and `canvas.palette {open: false}`, drawn as an NSPopover on the Mac and a popover (a sheet with detents in compact width) on the iPad.

### 12.10 Editing a value on a face (N4 slice 2)

The maintainer, after using the iPad app: "App looks good, cant edit text values." Every inline widget kind the scene describes (8.5: `number`, `toggle`, `choice`, `multichoice`, `text`, `multiline`, `path`, `color`) is edited on the face, in the field-editor pattern (plan 5.4): the widget at rest is drawn into the face, and while a row is edited one native control sits over it; it is made when the editor opens and taken away when it closes, never a control per row. Everything is a command (golden rule 4: "No exceptions to the rule"):

- **Which rows can be edited** is the core's (`inline::editable`): a row whose `source` is `own` with a widget of one of those kinds. A wired row (its value comes from the wire), a socket-only one, an inherited one (a bundle's) and a read-only one can't, and `row.edit` and `row.set` say why.
- **`row.browse {node, port}`** (B133) asks for a path input's file by its hints, closing any field editor first without its text, then sets the answer as typing does (one `node.set`); cancelled, nothing changes. What the shell's answer must be is in 11.2 (purpose `path`).
- **`row.edit {node, port}`** opens the editor, the core's state, published as the scene's `view.edit: {node, port, face, row}` (left out while none is open); `{step: 1 | -1}` is Tab and Shift-Tab (the next or previous editable row: a face's rows top to bottom, faces left to right a column at a time, wrapping); `{open: false}` is Escape. A tap on an editable row opens it (8.13), so does a script; the shell follows `view.edit`. It closes on `graph.new`, `graph.open` and when its node goes.
- **`row.set {node, port, text | value}`** is what a commit sends: Return, Done, the field losing focus, a menu's pick, the colour picker closing, a scrub's end. The core checks it by the row's widget (control protocol: parsed, made whole, held to the range with a `note`, a choice one of its own and not disabled, a colour in the port's format) and makes it one `node.set`, one undo step named "Set Value" (none when nothing changes). A refused value changes nothing; the shell keeps the editor open and shows the core's words. A shell never commits intermediate values (8.5); a scrub's are shown, not set.
- **Where the control goes** (the Apple kit's `EditorPlacement`): over the widget's rect at the canvas's zoom, its text the face's size (11 graph units times the zoom). Zoomed out so far that the text would be smaller than legible (10 points with a pointer, 11 on the iPad), the control is drawn at that legible size instead, centred on the row and floating over the face with a shadow (at least 170 or 220 points wide, 20 or 32 high), so editing never moves the view. A control near the view's edge is moved in. It follows the row when the view pans or zooms.
- **The Apple controls:**

| Kind | macOS | iPadOS |
|---|---|---|
| `text`, `path`, `multichoice`, `number` | NSTextField over the row: Return commits, Escape cancels, Tab and Shift-Tab commit and move, focus lost commits; no spelling, quote, dash or text substitution; a number right-aligned in the monospaced digits | UITextField: the keyboard for the kind (numbers and punctuation for a number, so a minus and a point are on it; URL for a path), no autocorrection, capitals, smart quotes or dashes; a bar over the keyboard with Previous, Next and Done; Return and Done commit; a hardware keyboard's Escape, Tab and Shift-Tab as the Mac's; focus lost commits; the canvas scrolls so the row stays above the keyboard |
| `multiline` | NSTextView in a scroll view, monospaced: Return is a new line, ⌘Return commits | UITextView, monospaced: Return is a new line, Done or ⌘Return commits |
| `choice` | NSPopUpButton with the core's choices (a disabled one greyed, its reason the tooltip); its menu opens at once for a person's click, and after Tab or a script waits for a click or Space | the system's pull-down menu: a touch on the row goes to a button over it whose menu is the choices (the one set ticked, a disabled one greyed with its reason); a pick is `row.set` |
| `toggle` | A checkbox on the face; a click flips it in the core | A switch on the face; a tap flips it in the core |
| `color` | NSColorWell; the colour panel opens for a person's click; committed when the panel closes | The system colour picker in a popover; committed when it closes |
| scrub | A drag on a number's row (the kit's `CanvasDrag`, kind `scrub`): the value shown over the row as it moves, one step at the end | A one-finger drag on a number's row, the same |

The more-button (⋮, the header's `menu` part) opens the node's menu from the core: on the Mac a click's `ui.gesture` tap replies with the node's menu, shown as the context menu is; on the iPad a touch on it goes to a button over it whose menu is the core's (`menu.get` for the context the tap gives), the system's menu rather than a sheet.

### 12.11 Faces never overlap (N4 slice 2, decision 13)

The maintainer, N3 exit card 13: "Nodes that grow after a run overlap their neighbours: Fix in N4." The core keeps measured faces apart in the scene (`scene::make_room`): where two overlap, the later one (by top, then left, then id) is shown out of the way, to the right when it stands to the right of the other and that is the shorter way, else below, 16 graph units clear (`ROOM_GAP`); a face pushed may push the next (at most 8 passes). The push is the view's, not the graph's: a face's `pos` in the scene is where it is shown, its node's position is untouched, so a run that grows a face adds no undo step and leaves a clean document clean, and when the face shrinks again it goes back. A drag moves a face from where it is shown (its `edit.move` names the shown place plus the offset), so a pushed face doesn't jump when it is dropped; the hit test, wire paths, open groups' rects, the fit and `geometry.locate` all use the shown places. Faces not yet measured are left where they are. Frames don't take part in the push itself; since N4 slice 3 a frame is shown grown to hold the members pushed (12.17). The egui editor draws its faces where the graph puts them (egui-snarl owns their positions), so it doesn't follow this.

### 12.12 Node icons (N4 slice 2)

The node icons are the Lucide set the core embeds (`plumcraft_editor_core::icons`, which the egui editor now reads too); `icon.get {names}` gives a shell their SVG text, and a name the set lacks comes back as the fallback's (`box`). The Apple canvas parses them (paths, circles, rects, lines, polylines, arcs) once into Core Graphics paths and strokes them in the header, after the state's spot; a pack's own icon is never replaced by an SF Symbol. The header also has the more-button at its right end (three dots).

### 12.13 What the Apple shells keep to themselves (N4 slice 2, replaced in slice 3)

Slice 2 wrote the iPad's Select mode and Share… down here as the shell's. **The maintainer did not accept it: "No exceptions to the rule."** Since N4 slice 3 both are core commands (12.18), and the rule is this: a user action implemented in a shell is a defect. What a shell keeps is only the view's pan and zoom (5.3: smart zoom, the scroll and pinch; Zoom In, Zoom Out and Actual Size as gestures of the view, though since N5 slice 3 their keys are the core's `canvas.zoom`, below) and platform chrome with no document meaning (full screen, the Window menu, Minimize, a window's own close and resize, the document browser and Documents, the system's text editing in a focused field). Slice 3's audit of both apps and the kit (the report, `claude-debug/native-n4-slice3.md`) found the menu model's items the shells did themselves (12.16) and the toolbars' own states (12.18), and made each a command.

**Zoom In, Zoom Out and Actual Size (N5 slice 3).** `canvas.zoom {by} | {to}` is a canvas command, as `canvas.fit` is: the core checks it (a factor 0.1 to 10, or a zoom 0.1 to 4, exactly one) and keeps it as view state, the scene's `view.zoom: {seq, by?, to?}`, which the shell follows once per `seq` about its view's centre; not an undo step, in no menu (the Windows app binds Ctrl+=, Ctrl+- and Ctrl+0 to it; the Mac's ⌘+ ⌘− ⌘0 and egui's keys are their own still, and both ignore `view.zoom`). A conformance case covers it ("Zoom In, Zoom Out and Actual Size are the core's commands", read half).

### 12.14 Windows: the parameters inspector and pop-outs (N4 slice 3)

12.5's `windows` domain is carried, as built, in the scene's `view` (as the palette and the field editor are, so a shell follows it with the scene it already fetches): `view.windows: {parameters?: node, popouts?: [{id, node, port, index, title, viewer?, raised, stamp}]}`, left out while none is open. `node.parameters {node}` and `{}` open and close the inspector; `view.popout`, `view.close` the pop-outs (`MAX_POPOUTS` 16); a pop-out asked for again while open is brought forward (`raised` rises); `stamp` changes when what `popout.get` reads changes (a run's value, the viewer's state), when the shell reads it again. A new graph (`graph.new`, `graph.open`) closes them all; a node removed closes its inspector and pop-outs.

- **The Watched column is gone from the Apple shells** (the maintainer, N3 exit card 14: "Eliminate the side column in favor of just the popout window"). A tap still watches (`run.watch`: the face's watched border, what a run watches by default); a double tap on a value, Pop Out in a menu, or a script opens it in a window of its own.
- **What a pop-out shows:** `popout.get {id}` → `{id, node, port, index, title, viewer, stamp, summary, media, value?, data?, picture?, excerpt?, error?, text: {wrap, mono, all}, fit}`: the value as 13.2's `watched` has the panel's, with its viewer's state. The viewer's controls are the core's commands (13.2): Fit and 1:1 `viewer.zoom {to, popout}` (kept as the pop-out's `fit`), Wrap, Monospaced and Show All `viewer.text {popout, wrap?, mono?, all?}` (new). Viewers built: `picture`, `text.plain`, `scalar`; other kinds say "no viewer yet" (slice 5).
- **macOS:** a real window per pop-out: titled with the node and port, in the Window menu, its frame remembered per value (`frameAutosaveName` by `node.port`, not in a scripted launch), closed with its close button or ⌘W (`view.close`); in front for a person at the active app, else ordered just above its document's window, so a script never brings one in front of another app or takes the keyboard.
- **iPadOS:** since N4 slice 5 a sheet with medium and large detents by default (the graph in sight above it at medium), one value at a time (another replaces it and closes the one replaced, so the screen and the core agree), with Done (closing at once, the core told after), a swipe down, and Open in New Window; a scene of its own (an activity `com.plumcraft.pad.popout`) only when the person asks for one, with a Close button in its bar that destroys the scene and brings the graph's scene back to the front, as closing it with the window's controls does. (Slice 3 opened a scene whenever the graph's wasn't full screen, which on iPadOS 26's windowed apps is always: the system showed it over the graph, the maintainer: "No way to close popout or get back to the main canvas".)
- **Not persisted.** Pop-outs and the inspector are the document's view state, not the graph's: they close with the document and don't come back on reopen (as the egui editor's). A decision for the maintainer (the report).

### 12.15 The parameters inspector (N4 slice 3)

`parameters.get {node?}` gives every input of the node, shown on its face or not, in egui's sections (Inputs, Parameters, each settings section, More fields): each row the face's row (`InRow`), `onFace`, `canHide` (a wired input always shows), `editable`, `default`, `reset` and `help`; `hidden` lists what the face leaves out. Edits are the face's: `row.set` takes any input of the node since this slice (it read the face's shown rows before), and `{reset: true}` sets the default (↺). **Show on Face** is new: `node.face {node, port, shown: true | false | null}` (a session command; the node's `face` map in the workflow, saved, not an undo step, as `node.fields`), honoured by every shell's rows (`rows::shown_rows`, which the egui editor and the scene share). **B45 (fixed after N4):** egui built its rows from `input_rows` less the tucked fields while the scene left out the rows `node.face` hid, so with a row hidden egui's pin `i` was not the scene's row `i`: labels, fields and pins slid out of line and an edit changed the wrong port. egui now builds its rows with `shown_rows`, so pin `i` is the scene's row `i`, and a row whose key isn't the scene's for that pin draws no field. egui's parameters window has a Show on Face box per row (`node.face {node, port, shown}`; disabled on a wired input). The Mac shows the inspector in a popover on the node (native controls: a slider and a field for a number with a range, a switch, a pop-up, a field, a text view, a colour well; the eye and ↺ per row); the iPad in a popover in regular width and a sheet with detents in compact width, an inset-grouped list. Opened by a double tap or click on a node (8.13), the node's menu (⋮ or the context menu), Node › Edit Parameters… (⌘I, the core's item on the one node selected) and scripts.

### 12.16 Names edited in place, and the menus' last shell items (N4 slice 3)

Before this slice the menu model had items with no command that a shell did itself (`rename.node`, `rename.group`, `rename.frame`, `rename.net`, `rename.port`, `name.port`, `wire.slice`'s Slice…, `output.label`'s Label All Uses…, `group.editPromoted`, Go to Driver and Next Use, Add Bend Point without a point, Draw All as Wires). The Apple apps did nothing for them. Every one is a command now:

- **`name.edit {target}`** opens an in-place name editor, the core's state (the scene's `view.name: {target, text, hint, label, rect}`), `rect` in graph units where the shell puts its text field (a face's title, a frame's tab, a group's band, a net's driver tag, an input's row, a wire's middle); `name.set {text}` commits it through the command that owns the name (12's table in the control protocol), one undo step, and closes it; `{open: false}` is Escape; a refusal leaves it open with the core's words. Targets: `{node}`, `{group}`, `{frame}`, `{net, group?}`, `{port: {node, port}}` (name, rename or, blank, unname an input's parameter name), `{slice: {node, port}}`, `{label: {node, port}}`, `{promoted: {group, label}}` (its label; its doc and range stay `group.editPromoted`'s, a form not built). The Apple shells put one native text field over `rect` (as a row's field editor, 12.10); the egui editor shows a small window for it.
- **Go to Driver** is `canvas.reveal {node: driver}`; **Go to Next Use** `canvas.reveal {net, group?}`, the core cycling the uses. `canvas.reveal` is the core's view state since this slice (the scene's `view.reveal: {seq, node, rect}`, as `view.fit`): before, it was a shell-only command the C ABI refused ("isn't available from this shell"), so the Apple apps could not reveal at all. **Draw All as Wires** is `wire.unlabel {to: [..]}`, one step. **Add Bend Point** puts the point where the context gesture was (the gesture's menu carries `at`), else midway.
- **Double taps (8.13), "open what it is on":** a face its parameters; its preview or text excerpt, or an output pin, or a wired input's pin, pops the value out (`view.popout`); a frame's tab and an open group's band edit their name (`name.edit`); a tap on an open group's band chevron (its first 26 units) collapses it (`group.collapse`).

### 12.17 Groups, frames and nets in the Apple shells (N4 slice 3)

Groups are made from the selection (Group › Group, ⌘G, the node's menu), ungrouped, collapsed and expanded (the chevrons; Expand in the proxy's menu; since B156 also Collapse in the open group's band menu, and Group › Collapse Group or Expand Group, for the selected nodes' group or the selected collapsed group, each `group.collapse` and one undo step), entered (a double tap on a collapsed group, the band's pencil, `canvas.focus`) and left by the way back the core's `focus.path` gives: on the Mac a path control over the canvas (Graph › outer › inner, each `canvas.focus`); on the iPad a capsule of the same crumbs over the canvas. Names are edited in place (12.16); group ports are the node menu's Promote › and Expose › and the proxy's outputs, as before. Frames are made round the selection (Edit › Frame Selection), renamed in place, resized by the handle, moved with what they hold, coloured and removed from their menu. Nets: made by Label All Uses… or a wire's label, joined (`net.connect`, a tag dropped on an input), left (Draw as Wire), renamed in place; a net's driver tag says how many inputs it feeds (`driver.text`, "picture ×3", the egui editor's words; the hit test's estimate is that text's). Named ports: Name This Port…, Rename Port… and Remove Name on an input's menu, the name edited in place over the row.

**Frames and the push (12.11).** A frame grows, in the scene only, to hold every member the push moved: right and down to the pushed face's rect plus 16 (`FRAME_GROW_PAD`); its size in the graph is untouched, and what it holds is still what its own rect holds, so moving it moves the same things. The growth goes when the push does.

### 12.19 The file commands of an autosaved document (N4 slice 4)

`graph.saveCopy {path}` (the graph written elsewhere; the document keeps its file and its dirty state; `ui.invoke` asks `file.save {purpose: "copy"}`), `graph.duplicate {}` (a copy beside the file with its extension, "Name copy.plum", "Name copy 2.plum", …), `graph.rename {name}` (the file renamed in its folder; a bare name keeps the file's `.plum` or `.json`, else gets `.plum`; refused over a file already there), `graph.move {folder}` (`ui.invoke` asks `folder.pick {purpose: "move"}`). Each replies with `graph.save`'s summary, `copy` for the first two; unsaved changes stay unsaved. In no menu: the iPad's title menu offers Duplicate, Rename (the system's, in the title), Save a Copy…, Move… and Share…, and no Save.

### 12.20 Watching shows the value (N4 slice 4)

The maintainer, of slice 3's Mac build: "No watcher popout at all." With the Watched column gone, a tap on a pin and every Watch item ran `run.watch` alone, and nothing showed the value. A document whose shell has no Watched panel (`Document::watch_pops_out`, set by the C ABI's core for every document) pops a watched port's value out: `run.watch {node, port}` also runs `view.popout {node, port}` (an open one is brought forward). `run.watch {node}` (a title, a preview) still selects and watches without a window. The egui editor, which has its panel, is unchanged.

### 12.21 Value editors' data, and the curves editor (S4a, 2026-11-23)

The maintainer: a general curve editor in the shell, used by any engine that names it; the curve maths ported to the shell for drawing and pinned by shared vectors, every edit a core command; curves before masks; the iPad and the Mac together, Windows and the iPhone later (decisions 1A, 2A, 4A, 5A, 6A; re-run while dragging not decided: none, as the egui editor).

- **The curves value** (`plumcraft_proto::curves`, the one model): `{"master" | "red" | "green" | "blue": {"points": [{"x", "y"} | [x, y]], "enabled"}}`, as an object or as that object in a string; points in 0..1, sorted by x; a missing curve is the identity; channel k goes through the master curve, then its own. The curve through a channel's points is cubic Hermite with secant slopes, clamped to 0..1, flat beyond the ends. `crates/proto/fixtures/curves.json` pins it: every copy of the maths and every shell's port must give its y values exactly (compare the bit patterns).
- **An engine names the editor** with `NodeDef.editor {kind: "curves", port}` on an input holding a curves value. The array engine's Apply Curve (`array.apply_curve`, its `curves` input) does; so may any engine.
- **What a run sends it** (`NodeReport.editor.curves`; in the Rust SDK, a node's `_editor` output `{"curves": …}` and its `_editor_picture` array, `plumcraft_engine::exec::EDITOR_KEY`): `{names: [channel name] (1 to 3), histograms: [[count; bins]] (one per name, over 0..1 as the curves see the values), bins?: 256, live: bool (an 8-bit preview through lookup tables shows what the curves do), picture?: rgba8 rendition (the values as the curves see them, at most 1024 a side), error?}`. Every field is optional to a shell: an editor with no data shows the curves without a histogram or preview.
- **`node.editorData {node, kind?}`** (a command, read only, not journaled) returns `{node, kind, port, value, wired, data}`: the value the editor edits (the port's; `wired` when a wire feeds it) and what the node's last run sent its editor (`kind` defaults to the one the node names), `data` null before a run. A shell reads the picture through `media.get` with its handle, as any picture.
- **The edits** are core commands, one undo step each (`docs/control-protocol.md`): `curves.edit {node, port?, channel, points, enabled?}` (2 to 64 points in 0..1), `curves.reset`, `curves.preset {preset}` (Light S, Medium S, Strong S, Inverse S), `curves.equalize` (points at the quantiles of the channel's histogram from the last run; refused before one). `port` defaults to the editor's; `channel` is 0 (master) to 3 or its key. A wired port is refused. The value keeps its form (a string stays a string).
- **The Apple shells' editor** (`PlumCraftShell`: `CurvesEditor`, registered under `curves` in `NativeEditors`): opened by `node.editor` (the node's footer and menu, FR1's row on a node or a collapsed group's face) while the scene's `view.windows.editors` names the node, presented as the value window is (a sheet on the iPad, a window on the Mac); a kind with no native editor gets the core's value window (the value as JSON, 9A). Tabs All and each channel (an edited one marked), On, Reset, Presets and Equalize; the plot with the channel's histogram behind and the other curves as ghosts. A press on empty space adds a point, a drag moves it between its neighbours, a drag well outside the plot or a double tap removes it (two points are kept); a Mac click does the same. The drag is the view's; one `curves.edit` goes when the finger or pointer lifts. The view follows the value from elsewhere (an undo) except during a drag. With `live` data and a picture, the preview passes the picture through the curves' 8-bit lookup tables; without `live`, it shows the picture as it is, captioned as the last result (the curves show in it on the next run); a picture that doesn't load says why. On the iPad the editor opens at the sheet's large detent, the plot and the whole preview in view. Each point is an accessibility element whose value says where it is, adjustable by steps of 0.01 in y (each step a `curves.edit`).
- **Not yet:** Windows and the iPhone's compact layout (their own slices); the mask editor (S4b).

### 12.22 The mask editor's core half (S4b slice 1, 2026-11-23)

The maintainer's decisions (S4b): a new array node, Paint Mask, "which also takes a mask input" (1A); a Swift port of the brush pinned bit for bit (2A); the mask at the picture's size up to 2048 on its long side (3A); with nowhere to write, ask for a folder once and remember it (4A); prune unreachable edit files ("Ref commonkit -- both", 5B); write the file in the stroke command (6A); the sidecar says `plumcraft` (7A); one finger paints, two pan and zoom (8A); a point may carry a pressure, checked and not used (9A).

- **The model** is `plumcraft_proto::masks`, toolkit-free: the brush (Mask, Erase, Paint; radius, hardness, flow, density, Paint's grey), a stroke into dabs a quarter of the radius apart, coverage building to the density, the canvas (the picture as painted and the mask), its rounding to 8 bits and the pixel hash an edit file's name carries. `crates/proto/fixtures/masks.json` pins it (11 strokes on a 48 × 32 picture: the file's bytes, the hash, and a hash of the unrounded mask's bits, which a drawing port must match).
- **An engine names the editor** with `NodeDef.editor {kind: "mask", port}` on a string path input; the array engine's Paint Mask (`array.paint_mask`: `picture`, optional `mask`, `edit`) does. Its run sends `NodeReport.editor.mask = {w, h, picture, base?}`: the mask's size (`plumcraft_proto::masks::canvas_size`: the picture's own up to 2048 on its long side, else scaled down, rounding), the picture at that size (`rgba8`), and, when a mask is wired in, that mask as a grey `rgba8` (`base`: where the painting starts). The Rust SDK sends a second picture from an output keyed `_editor_picture:<name>`.
- **The edit** is the input's value: the path of an RGBA PNG at the mask's size, alpha the mask and the colours the picture as painted, named `<node>-<16 hex of the pixel hash>.png`, with a sidecar `<file>.json` `{call, w, h, node, editor: "plumcraft"}`. A canvas no different from where it started is no edit (the value empty). Paint Mask reads the file, scales the mask to the picture (bilinear) and lays in the painted colours where they differ from the picture it was painted on; with no edit, a wired mask passes through. Once there is an edit it holds the whole mask, so a later change to the wired mask doesn't reach it until the edit is cleared (the reading of 1A recorded in note 24).
- **Where files go:** `<document>.edits/` beside the document when it can be written; else the folder kept in the graph's `meta.mask_folder` (`mask.folder`). With neither, `mask.stroke` fails with "no folder for mask files…"; invoked (`ui.invoke`, a person's stroke), the core asks `folder.pick {purpose: "edits", title, why}`, keeps the answer (`mask.folder`, an undo step) and strokes. A value inside the document's folder is stored relative to it; Save As re-anchors it (`rebase_paths`).
- **Commands** (`docs/control-protocol.md`): `mask.stroke`, `mask.clear`, `mask.get`, `mask.folder`; a stroke and a clear are one undo step each, refused for a wired port and (a stroke) before a run.
- **The Apple shells' editor** (S4b slice 2; `PlumCraftShell`: `MaskEditor.swift`, registered under `mask` in `NativeEditors`): opened by `node.editor` (the node's edit row, a collapsed group's promoted row) as the value window is (a sheet at full height on the iPad, a window on the Mac). Controls: Mask, Erase, Paint; Size, Hardness, Flow, Density, and Paint's Grey; Show Mask; Fit; Clear (`mask.clear`). The canvas is the picture with the mask over it as a blue tint (55% of the mask). On the iPad one finger or the Pencil paints, two fingers pan and pinch to zoom, and a two-finger tap undoes (it waits for a pinch or a pan to fail); on the Mac the pointer paints, the scroll wheel and trackpad pan, ⌘-scroll and a pinch zoom, `[` and `]` change the size, and Undo in a window the core keeps undoes the document (the window's delegate answers `undo:`). Before a run it offers Run. During a stroke the view paints with the Swift port (bit for bit the core's) and redraws only the area the dabs touched; on lift it invokes one `mask.stroke` as the person's (a document with nowhere to write asks for a folder, with the platform's folder picker). The core's file is then the canvas: the view keeps its own result when the file's name carries its pixel hash (the same pixels), and a canvas it has had comes back from memory on undo; an edit opened from disk is read by the shell's own PNG reader (`PNGRGBA`: the platform's decoder premultiplies alpha, which loses the picture's colours where the mask is 0).
- **Clean Up Mask Files** (`mask.prune`, decision 5B, commonkit 0.3's `prune_folder`): in this document's own edit folders only (`<document>.edits/` and the one kept by `mask.folder`, each only if it exists; a folder that is a symbolic link is refused), the mask files (`<node>-<16 hex>.png`) whose sidecar says `plumcraft` for that node, that nothing reaches (the graph, any graph undo or redo can bring back, a JSON file beside the document) and that are older than a day (`age_s`), go with their sidecars. Another editor's files and files with no sidecar are left. `mask.prune` is a dry run unless `dry_run: false`; invoked for a person it asks first with the count (prompt `pruneMasks`: Remove, Cancel).
- **Not yet:** the folder bookmark kept across a relaunch on the iPad (slice 3), the private app (slice 4).

### 12.23 A graph's file name: `.plum`, and `.json` still (FR2, 2026-11-24)

The maintainer: "change the default graph file extension from .json -> .plum … Continue to accept .json as an alternate. Register .plum extension on platforms where it is not registered already". The rule is the core's, in one place (`plumcraft_session::graph_file`), and every shell reads it from there:

- **`graph.fileTypes {}`** (a command, in no menu) → `{default: "plum", extensions: ["plum", "json"], label: "Graph", untitled: "untitled", kinds: [{label: "Graph", extensions: ["plum"]}, {label: "Graph as JSON", extensions: ["json"]}]}`. A shell asks once after the core opens, for what it names itself (the iPad's new graphs) and what its browser lists.
- **New graphs, Save As, Save a Copy:** the core's `file.save {purpose: "workflow" | "copy"}` names the graph's stem (or `untitled`) with `.plum` and offers `kinds` with `.plum` first and `.json` still a kind (a panel with a format choice shows both; one without takes the first).
- **An existing `.json` graph keeps its name:** Save writes to its own file and asks nothing; `graph.rename` with a bare name keeps `.json`; `graph.duplicate` keeps the file's extension.
- **Open:** `file.open {purpose: "workflow"}` lists `plum`, `json`, `zip` and an application's openers' extensions. Reading is by content: a `.plum` file is the same workflow JSON, and a file of any other name a person opens is read as before. An application's opener can take neither graph extension.
- **Bundles:** the graph at a bundle's top may be `.plum` or `.json`; a new graph's bundle holds `<name>.plum`.
- **The platforms:** each app declares `.plum` as its own type and `.json` as one it opens without claiming it:
  - **macOS and iPadOS:** `UTExportedTypeDeclarations` for `com.swapwizard.plummer.graph` (conforming to `public.json`, tag `plum`, MIME `application/x-plumcraft-graph`), and `CFBundleDocumentTypes` with that type as Owner and `public.json` as Alternate. An application built on the Mac or iPad shell declares the same type in its own Info.plist (imported, `UTImportedTypeDeclarations`, or exported when it is the only app it ships with) and lists it in its document types; the kit's `GraphFiles` (`PlumCraftShell`) has the identifier.
  - **The document icon** (the maintainer, 2026-11-24: "Yes, draw a document icon"): a page with a folded corner carrying the app's plums, drawn per size (the app's tile at 128 px and up, one flat plum at 32 and 64, a plum disc at 16). The Mac names it for the type and the document type (`UTTypeIconFile`, `CFBundleTypeIconFile`: `PlumGraph.icns` in the app's resources). The iPad declares none: Files shows a graph's thumbnail or the app's icon and doesn't reliably take a declared document icon; a thumbnail extension would be the way there. Windows takes `assets/doc-icon/plum-document.ico`.
  - **Windows:** a file type association for `.plum` (the Windows shell's own slice).


### 12.24 File › Open Recent is the core's (B166, 2026-11-24)

The maintainer: "open recent never has a list of recent graphs". The Mac's Open Recent was AppKit's: a submenu holding Clear Menu, which the document controller fills. The Mac builds its menu bar anew whenever the front document or the model's shape changes, and AppKit fills only the recent-documents menu it adopted first, so the list stayed empty while AppKit had the graphs. Now the list, its menu and its items are the core's, and every shell draws them from the model:

- **The list:** the graphs last opened or saved under a file of their own (`graph.open`, `graph.save`, `graph.saveAs`, `graph.rename`, `graph.move`; a rename or a move takes the old name's place), newest first, once each, at most ten, kept in the application's preferences (`prefs.json`'s `recent`, beside its other preferences, so an application with its own name has its own list) and shared by a core's documents. The egui editor keeps it in its own preferences the same way.
- **The menu:** File › Open Recent (`id: "menu:File/Open Recent"`), after Open…: one item per graph, labelled with the file's name (and its folder's when two share a name), its whole path as `hover`, disabled with why when the file is no longer there; a separator; Clear Menu, disabled with nothing to clear.
- **The commands** (in no menu of their own; the items run them with `command.run`): `graph.recent {}` → `{recent: [{path, name, exists}]}`; `graph.openRecent {path}`, refused for a path not in the list or a file no longer there; with a window per document (`platform.document_windows`) the graph opens in a window of its own unless this one is untouched or already holds it, else here, after the unsaved-changes prompt when there are any; `graph.clearRecent {}`.
- **The Mac** keeps noting graphs with AppKit (`noteNewRecentDocumentURL`) for the Dock's menu and the system's recent items. With no document open, its bar has no model to ask: File › Open Recent there shows the last model's items, each opening its graph as Open… does, and Clear Menu waits for a document.
### 12.18 Select mode, Share and the toolbar (N4 slice 3)

- **`canvas.selectMode {on?}`** (a toggle): the document's Select mode, the scene's `view.select`, View › Select Mode's check, the canvas menu's item. A touch shell makes a one-finger drag on empty canvas a marquee while it is on; a script sets it; on the Mac it changes nothing (a drag there is a marquee already) and the Mac's toolbar leaves it out.
- **`graph.share`**: File › Share…; invoked, the `share` ask (11.2) with `{file, subject}`, which the Mac answers with `NSSharingServicePicker` and the iPad with the share sheet (only for the person's ask; a script's is the script's to answer); disabled with "save the graph first: Share sends its file" while the graph has no file.
- **The toolbar is the core's:** `menu.get {context: {kind: "toolbar"}}` lists the buttons (Add Node, Fit, Run, Run Selected, Cancel, Select Mode, Share), each with its label, `icon` (a Lucide name; the Apple shells show their SF Symbol for it), enabled state, reason and check, refreshed with the menus. Neither app keeps a button's state of its own.

## 13. Viewers

### 13.1 The registry as data

`state.get {domains: ["watched"]}` includes, for the panel and each pop-out, `viewers: [{id, label}]` offered for the value in order (the first is the default, `viewers::kinds_for`) and `viewer`, the one chosen (`view.viewer`). The registry today, in order of preference (`crates/ui/src/viewers/mod.rs:137-198`):

| Id | Offered when | Bytes it needs |
|---|---|---|
| `picture` | media `image` | `derive:image` of `full`, else the `rgba8` rendition |
| `svg` | media `vector` | `derive:svg` at the view's size (redrawn sharper when zoomed) |
| `pdf.pages` | `full` is `application/pdf` | `derive:pdf` per page, or `bytes` for a platform's own renderer (PDFKit, decision 9) |
| `mesh` | an STL (`mesh::accepts`) | `derive:mesh` at a camera, or `bytes` |
| `doc` | media `doc` | The rendition; Open in … (`doc.open`) |
| `array.plot` | an (N, k) table of positions | `f32` of `full` |
| `array.pictures` | media `ndarray` | Renditions with role `view`, captioned |
| `array.grid` | media `ndarray` | `f32` of `full` |
| `audio` | media `audio` | `bytes` of `full` (playback in the core), `derive:spectrogram`, `peaks` |
| `table.grid` | media `table`, or rows (a list of lists) | `Display.value` |
| `xml` | XML text or bytes | `bytes` |
| `text.plain` | media `text` | `bytes` |
| `scalar` | media `scalar` | `Display.value` |
| `yaml` | media `tree` | `Display.value`, formatted by the core (`viewers/yaml.rs`) |
| `tree` | `tree`, `table`, or an object or array value | `Display.value` |
| `items` | a list with a sheet | the sheet's thumbnails |
| `hex` | anything with `full` except a document | `bytes` |

An application adds viewers ahead of these (`viewers::register`); in a native shell a viewer it adds is drawing code registered by id in the Swift or Kotlin kit, with its model in Rust (plan 7).

### 13.2 State and tools

| State | Today | Contract |
|---|---|---|
| Probe, hover reading, zoom, `px` detail | `viewer.probe`, `viewer.zoom` (commands); `picture_tools.rs` | Commands; readings computed by the core from `full` (`probe.rs`), in `watched.view.tools` |
| Compare A/B, how, split, channel, result | `viewer.pin`, `viewer.unpin`, `viewer.compare`; `diff.rs` computes in the UI crate | Commands; the difference computed by the core (`plumcraft-media`), its picture a `derive:compare` |
| Sheet | `viewer.sheet` | Command |
| List item shown, array view (auto, heat, …) | `run.item`, `run.view` | Commands; a list of lists (interface 7.6 rule 2): `run.item {index, path}`, the run and the item within it, as the display's `path` and `levels` say (`popout.get` gives both, `levels` absent for a list that isn't nested) |
| Grid format, colour by value, slice, range | `viewers/grid.rs:335`, egui-local | Proposed commands `viewer.grid {format?, colored?, slice?}` (open question 5) |
| Table sort, colour | `viewers/basic.rs:311`, egui-local | `viewer.table {sort?, colored?}` |
| Text wrap, find | `viewers/basic.rs:153`, egui-local | `viewer.text {wrap?, find?}`; as built (N4 slice 3) `viewer.text {popout, wrap?, mono?, all?}`, a pop-out's; `find` not built |
| PDF page | `viewers/pdf.rs:149`, egui-local | `viewer.page {page}` |
| Mesh camera | `viewers/mesh.rs:341`, egui-local | Shell-local, reported in `ui.report` (a drag turns it per frame) |
| Array pictures zoom, pan, log scale | `array_view.rs:132`, egui-local | Zoom through `viewer.zoom`; pan shell-local; `viewer.array {log}` (built, B154 slice 1, a pop-out's) |

Every viewer's state appears in `watched.panelViewer.state` (and each pop-out's) as `ui.inspect` reports it today.

**As built (N3 slice 1): `ui.inspect`'s `watched` comes from the core** on every shell (`plumcraft_editor_core::values::watched_json`): `{node, port, summary, media, viewer, value?, data?, picture?: {ref, w, h}, excerpt?: {text, offset, totalChars, lines}, error?}`, null while nothing is watched and its value fields null until a run has sent the value. `viewer` is the core's first choice by media (`values::default_viewer`: `picture`, `svg`, `doc`, `array.pictures`, `audio`, `table.grid`, `text.plain`, `scalar`, `yaml`, else `hex`), the start of 13.1's registry moving into the core; a test holds the egui registry's first choice to it for those media. `value` is a small value inline (a scalar's), `excerpt` a text's first 4 KiB, `picture` the rgba8 rendition a native shell borrows (`plum_blob`, or `media.get {handle, want: "rgba"}` on the C ABI), `data` the full value's reference. The egui shell adds its panel's fields over the core's (its `viewer` is its panel's, `full` whether it holds the full value, and the rest of the table above). Not built: `viewers` (the list offered) and the `watched` domain of `state.get`. Pop-outs are built since N4 slice 3 (12.14: `popout.get`, `viewer.text`, the scene's `view.windows`).

### 13.2.1 As built (N4 slice 5)

The viewers' state is the core's, a pop-out's: `viewer.table`, `viewer.tree`, `viewer.audio`, `viewer.picture`, `viewer.grid`, `viewer.hex` beside N4 slice 3's `viewer.text` and the existing `viewer.zoom`, `viewer.probe`, `view.viewer`, `run.item` and `viewer.sheet` (each takes `popout`); `popout.get` carries it as `state`, with the viewers offered (`viewers`, the core's registry, `crates/editor-core/src/viewers.rs`, held by a test to egui's), the value's form, statistics, pictures, peaks and sheet. What a large value shows is paged by the core on the media service's workers: `viewer.data {popout, part}` (`table`, `tree`, `yaml`, `grid`, `hex`, `probe`), pending until read, with the number text the core's. **The numbers grid's window of columns is the core's (decision 26, N4 person gate: "Move it into the core"):** `viewer.grid {popout, col?, cols?: "next" | "prev" | "first" | "last"}` sets the first column shown, always a multiple of 64 (`col`: the window holding that column; `cols`: a step of one window), never past the last window when the array's shape is known (`viewer_state::last_window`); `popout.get`'s `state.grid` has `col` and `colsShown` (64); `viewer.data {part: "grid"}` reads that window (it no longer takes a column of its own: `cols` is refused as an unknown field) and reports `firstCol`, `endCol` (one past the last shown), `colsShown`, `prevCols` and `nextCols` (whether a step either way shows more), beside the rows' `start`, `end`, `page`, `pages` and `pageRows`. A shell draws the window's caption ("columns 64–127 of 150") and a previous and next button sending `viewer.grid {cols}`, disabled by `prevCols` and `nextCols`; it keeps no column of its own. The egui editor's pop-out grid does the same (its Watched panel, which has no pop-out, still scrolls every column). Conformance: "a wide array's numbers are shown 64 columns at a time, in the core's window". New derived kinds on the media service (10.2): `derive:waveform:<handle>@<bins>` (peaks from every sample), `derive:spectrogram:<handle>` (a picture, Hann 1024 / hop 256, at most 2048 columns), and, for `viewer.data` alone, `derive:table:<handle>`, `derive:sorted:<handle>@<column><a|d>` (a permutation, lent as little-endian u32s) and `derive:json:<handle>` (at most 64 MiB). `media.get` takes `want: "waveform" | "spectrogram"`. A value is taken out by `value.copy`, `value.export`, `value.share` and `value.saveAs` (the existing asks). **The egui editor (after N4)** draws a pop-out picture's channel from the core's `viewer.picture` state, with a picker (RGBA, RGB, R, G, B, A) that sends it; one channel shows as grey and RGB without alpha (`plumcraft_media::raster::isolate`, the core's). The Watched panel has no pop-out to keep a channel and offers none. egui's other viewers still keep their own state (the audit's D4, not taken on).

Where the line falls (golden rule 4): what changes what is shown (a sort, a page, an outline member opened, play, pause, a seek, the level, loop, a channel, Fit and 1:1, the viewer chosen, the item of a list) is a command; continuous motion inside a viewer (scrolling, a pinch or scroll zoom of a picture, a column's width, the playhead moving while playing) is the shell's, as the canvas's pan and zoom are. A zoom past a picture's own pixels asks for detail once (`viewer.zoom {to: "detail"}` for an array, a picture file decoded larger or an SVG drawn larger through `media.get`), as the egui editor's scroll-in does.

The Apple shells (N4 slice 5): `picture`, `svg` and an array's single picture in each app's own zoomable view (channel, a readout of the value's own numbers under the pointer or a tap, a click or tap pinning the probe, Fit and 1:1, detail on zoom); `table.grid`, `array.grid`, `tree`, `yaml`, `hex`, `audio`, `items` and `array.pictures` (several) in SwiftUI shared by both apps (`PlumCraftShell/Viewers.swift`: a `Table` with sortable columns, disclosure groups, AVAudioEngine fed a few seconds ahead from the lent samples; a scripted run never opens the audio device); a value whose viewer no Apple viewer draws (`pdf.pages`, `doc`, `xml`, a pack's `x.…` type) shows what it is (type, media type, size, summary) and the engine's picture, never a guess at its bytes. Every viewer has View As, Copy, Share… and Save As….

**A picture's zoom and view through a list's items (B159, 2026-11-24).** The maintainer: "scrolling through items in result does not maintain zoom and view of image". Within one pop-out a zoom and view, once made (a pinch, a pan, or a zoom asked through the core), stay through every item and every new value: the same scale in screen points per pixel of the value, and the same centre as a fraction of the picture; so items of one size show the same region at the same zoom, and an item of another size the same zoom about the same relative point. With nothing kept the picture fits, or shows 1:1 when the core's `fit` says so; Fit or 1:1 asked again forgets what was kept. The zoom and view stay the shell's (the line above: continuous motion is the shell's), kept by the shell's pop-out across the picture's view being made again for each item (`PictureZoom` in `PlumCraftShell`); what the core gives is every `viewer.zoom` asked of the pop-out, in `popout.get`'s `zoomAsk {seq, to, at}`, which the shell applies once as `seq` rises, so a script zooms a native pop-out as it zooms egui's. A shell applies no zoom before its picture's view has a size (fitting an unsized view gave the smallest zoom, 5%), and applies it when the size arrives. The egui editor keeps its zoom (relative to fit) and pan through a picture's new values, and an array's while its shape stays. Moving the zoom itself into the core, shared by several pictures, is note 26's slice 2. Conformance: "a zoom asked of a pop-out reaches its shell once, with where to centre"; iPad UI test `testItemsKeepTheZoomAndView`.

### 13.2.2 View modes and histograms (B154 slice 1, 2026-11-24)

An array's view modes (floor F51) and its channels' histograms reach a native shell through `popout.get`:

- **`viewModes`** `[id]`, when the value's wire type offers any (an ARRAY: `auto`, `color`, `stretch`, `clip`, `heat`, `labels`, `angle`), and **`viewMode`** `{id, params}`, the one shown (the engine's reply, else the one asked, else `auto`). The shell shows a View picker beside View As (each mode's word as the egui editor's tips have it) that sends `run.view {popout, view, params?}`; `angle` takes `{turn}` (one full turn in the array's units, 1 by default). The new pictures arrive with the cached run that follows, in `views` (several) or `picture` (one), and `viewMode` names the one shown. A new mode keeps the value's `data` and `picture` and changes only `views`, so a shell that keeps its drawn picture while "only the state changed" must count `views` as part of what it shows (found on the iPad when this slice was launched: the heat mode was named and the auto picture stayed). A mode the type doesn't offer is refused with the list.
- **`stats[]`** gains each channel's **`histogram`** `{bins, lo, hi, top, log, counts, heights}`: `heights` are the bars, 0 to 1, already on the scale `viewer.array {popout, log}` sets (ln(1 + count) when log), so a shell draws them as they are and computes nothing; `lo` and `hi` are the range's text for the axis's ends; `top` the largest count. `nan` (already there) is shown beside the range when not zero. `state.array.log` is the toggle's state.
- The egui editor's panel and pop-outs keep their own drawing of both (unchanged).

Conformance: "an array's view is chosen through the core and its pictures change".

### 13.2.3 Several pictures, colour by value, number format (note 26 slice 2, 2026-11-24)

- **Several pictures share one zoom and view** (an array shown as a picture a channel): a pinch or a drag on any of them zooms and pans all, Fit forgets it, 1:1 is a point a value pixel, a tap reads the value's numbers there (`viewer.data probe`) and pins the probe (`viewer.probe`). The zoom is the shell's, kept in the picture's own terms through a list's items (13.2.1, B159), and a `zoomAsk` is taken as for one picture. A pop-out's `stamp` moves when a zoom is asked, so a shell reads the pop-out again.
- **Colour by value and number format** are the core's state (`viewer.grid {colored, format}`, `viewer.table {colored}`; `popout.get`'s `state.grid {colored, format, formats}` and `state.table {colored}`), and the page carries what a shell draws: `tints` (0 to 1, or null) and, for a grid, `range`. A shell tints a cell from its background towards its accent by `tint`; it computes no range itself.
- **A table-flagged array is a table first** (`viewers::offered`): the Histogram node's `centre` and `count`, a node's points. Since FR6 slice 1 (13.2.6) its Pictures, Values and Chart are offered after its Plot and Table.

### 13.2.4 Interchange formats: CSV and SVG text (note 26 slice 4, 2026-11-24)

- **A value typed `text/csv`** (`DataRef.meta.media`, as the text engine's CSV file type and a pack's CSV types carry it) is offered **Table** first and **Text** second (`viewers::offered`), the choice View As (`view.viewer`). Its table is read on the media workers by the CSV reader the text engine uses (`plumcraft_proto::csv`: quotes, separators, line ends, typed columns; at most 1,000,000 rows and 4,096 fields a row, refused past them), then sorted, paged and tinted as any table. Nothing new in a shell.
- **A TEXT that is an SVG document** (it starts, after an XML declaration, comments, a doctype and white space, with an `<svg` element: only the start is looked at; text that merely looks like CSV is not sniffed) is offered **SVG** first and **Source** second. `popout.get` gives it a `handle` of its own content and `svgSource: true`: the text carried inline, typed `image/svg+xml`, its picture started on the media service at 2048 a side, so a shell draws it as it draws any SVG value (`media.get {handle, want: "svg", px: 2048}`). A text past 8 MiB, or one whose display holds only part of it, keeps Source.
- The egui editor's registry does not offer these (the core's additions, as SVG's Source is).

### 13.2.5 Charts: the core's display list (note 26 slice 5, 2026-11-24)

The maintainer's decision 1A (note 26): the core lays charts out, every shell paints them.

- **Offered:** a table array the core plots (`chart::plotted`: columns `x` and `y`, `x0 y0 x1 y1` or `x1 y1 x2 y2`, `centre` and `count`, or any (N, 2) table) is offered `array.plot` ("Plot") first, then Table. In `NATIVE`.
- **`viewer.data {popout, part: "chart", width?, height?}`** (points, 120 to 8192, 640 by 400 when absent) lays the chart out at that size: `{size, kind: points | segments | columns, sameScale, plot: [x, y, w, h], marks: [{kind: points, r, xy} | {kind: segments, width, xy} | {kind: rects, rects} | {kind: line, width, xy}], axes: [{side: bottom | left, title, ticks: [{at, text}]}], grid: {x, y}, font, drawn, rows, a11y: {title, summary, x: {label, min, max}, y: {label, min, max}}}`. Every coordinate is in the chart's own points from its top-left; `xy` is a flat list of x, y pairs, `rects` of x, y, w, h. Tick text is the core's number text. Positions whose spans agree within 20 times are drawn to one scale, y down; quantities y up; a histogram's bars from zero. Past 50,000 rows the marks are an even sample (`drawn` of `rows`).
- **A shell** asks for the list at its view's size (again when the size changes), draws grid, marks (in its accent colour), axes, tick text (bottom ticks below the plot, left ticks right-aligned before it) and titles, and gives the chart its accessibility from `a11y` (the Apple shells: label, value and an `AXChartDescriptor`). It computes no scale, tick or number. The egui editor paints the same list (`plumcraft_ui::viewers::plot::paint`, egui_plot retired for it).
- **A 1-D array** (note 26 slice 6) is `array.plot` too, `kind: line`: its values by index, one `line` mark reduced past four points a pixel column of the plot (M4: each column's first, lowest, highest and last value, in order), `points` the pairs sent.
- **A table's chart** (note 26 slices 5 to 7, the maintainer's six kinds; 2026-11-24): a TABLE, a value typed `text/csv` or rows is offered `table.chart` ("Chart") after Table (in `NATIVE`). Its spec is the port's, `viewer.chart {popout, mark?: histogram | line | column | treemap, x?, y?: [column], color?, size?, path?: [column], agg?: sum | mean | count | min | max, stack?, bins?, clear?}` (each column checked against the value's, a number-only role against its type; a stacked line is `line` with `stack`, a stacked column `column` with `stack`), kept in the graph's `meta.charts["node.port"]` (decision 3A: saved with the file, outside the result hash, one undo step a change). With none, the core picks from the columns' types (a column chart of the first measure, a column with fractions before one of whole numbers, by the first category). `viewer.data {part: "chart"}` lays it out from the spec and adds `{spec, chosen, columns: [{name, numeric}]}` for the shell's choices. More in the list: a mark's `color` (`#rrggbb`, a ten-colour categorical palette, a series each), `{kind: area, color, xy}` (a stacked band, its top left to right then its base back), `{kind: labels, labels: [{x, y, text, detail}]}` (a treemap's, white on the rectangle, drawn only where it fits), `legend: [{text, color, x, y}]` (a swatch 10 points square at x, y, the name 15 points right of it), and a treemap's `groups` (its first level, a colour each) with `axes: []`: a shell draws no axis lines then. The kinds: `column` (sums by category, in first-seen order, 60 at most and the rest "other"; count with no y; side by side, or stacked by colour), `histogram` (bins over x's range, 30 by default; stacked by colour), `line` (y columns, or the first split by colour, against a numeric x or the row; the rows of one x made one point by `agg`, sum by default; reduced like a series; stacked: bands summed per x), `treemap` (one to three category columns, each group's area its share of size, or the rows counted; squarified; a nested group's label says its first level beside its total). A shell shows the choices as menus above the chart (the Apple shells: kind, X, Y, Colour; Groups and Size for a treemap), each one `viewer.chart`, and lays the chart out again after.
- **Zoom, selection and readouts** (note 26 slice 8, 2026-11-25): each the core's. A column of ISO dates or times (or typed `DATE`, `TIME`, `TIMESTAMP`) is a time axis (`timeX`), ticked by the calendar; a stacked chart's `normalize` draws shares (`… of shares`, ticks in %). `viewer.range` is the part shown, the pop-out's own state; the list then has `clip: true`, and a shell draws the marks clipped to the plot. `viewer.select` is the value's selection (decision 7A), shared by every window of the value (`view.popout {another: true}` opens its table beside its chart); the chart draws the selected rows' marks over the rest in orange and a brush as a band (`selection.x`), the table marks its selected rows and can show only them (`viewer.table {selectedOnly}`). A shell turns gestures into these commands through the list's `scale` (points to data): a drag across the plot is a brush of the x; a pinch (Apple) or the wheel (Windows) zooms the x about the pointer; Reset Zoom while `state.chart.range` is set; Clear Selection while `selected` isn't null; the pointer over a mark (Mac, Windows) or a tap (iPad) asks `hit` and shows its text. Nothing is scaled, binned or read in a shell.

### 13.2.6 Every view an array's kind and rank allow (FR6 slice 1, note 30, 2026-11-25)

The maintainer, 2026-11-25: "For each port, I should have the full range of visualizations available directly", and "In general, it's the user's job to deal with odd, and your job to make seeing odd things easy." So `viewers::offered` offers an array (`media` `ndarray`, or a TABLE carried as an array) **every view that can be computed for its dtype and rank**, never one withheld because its result may look odd:

- **Pictures** and **Values**, always;
- **Plot** (`array.plot`), when it plots: a numeric 1-D series, or an (N, k) array whose columns name a plot (`x0 y0 x1 y1`, `x y`, `centre count`) or that has two columns (`plumcraft_chart::plottable`), **flagged a table or not**;
- **Table** (`table.grid`) and **Chart** (`table.chart`), when it reads as a table within the reader's limits (`Table::fits_array`: any numeric array, its leading axes flattened into rows and its last axis the columns; at most 4,096 columns, 50,000,000 rows, 512 MiB). A plotted array has a Chart beside its Plot: the "one chart a value" assumption of note 26 12.2 item 9 is withdrawn.

A flag only orders them: a table-flagged array, or a 1-D series, shows Plot (else Table) first; any other array shows its Pictures first. Reading an array as a table is one rule for every reader (`plumcraft_media::table::Table::of`: the core's pages, its charts, `value.export` and the egui editor), and `viewer.chart` takes an array's columns as a table's (`chart_cmds::columns_of`). `viewer.data {part: "chart"}` draws the pop-out's own viewer: Plot's display list for `array.plot`, the table chart for `table.chart`. The egui editor's registry orders an array's viewers by the core's list (`viewers::kinds_for`), after any an application registered.

### 13.3 Fallback

A shell that lacks a viewer shows the engine's rendition as a picture (every value has one, `docs/interface.md` section 10) with the summary, and lists the viewer as offered but unavailable. `viewers::viewable` stays the rule that no value is shown only as hex.

### 13.4 Audio

Playback is in the UI today, not journaled (`crates/ui/src/playback.rs`, cpal). It moves to the core behind `AudioOut` (plan 4.4): `ui.audio {play, loop}` and the panel's Play, Stop and Loop are the same calls; position, output and errors are in `watched.audio.playback` as today. On iOS and Android the shell sets the platform's audio session and focus before the core plays (open question 6).

## 14. Themes and text

### 14.1 `theme.get`

```json
{"gen": 9, "name": "paper", "label": "Paper", "dark": false, "follow": true,
 "tokens": {"canvas": [250, 248, 244, 255], "panel": […], "…": […]},
 "fruits": {"cherry": […], "…": […], "olive": […], "plum": […]},
 "families": {"pinned": {"vision": "cherry"}},
 "radius": 6, "spectrogram": [[…], …],
 "text": {"base": 14.0, "title": 14.0, "port": 13.0, "caption": 12.5, "small": 12.0, "tag": 12.0, "semibold": true}}
```

- **Tokens**, by the names Settings and `theme.setToken` use (`theme::TOKENS`, `crates/ui/src/theme.rs:689`): Surfaces `canvas panel section field separator banner placeholder`; Text `text text_dim text_faint header_text accent error warning`; Controls `field_border widget_inactive widget_hovered widget_active`; Run states `state_queued state_running state_done state_cached state_error state_skipped state_cancelled stale highlight`; Media `chart waveform waveform_bg playhead`; Canvas `wire grid pin_default default_header default_body`. Also, not user-settable: `pin_hidden`, `faded`, `group` (header and body), `family_body_mix`, `fruit_l`, `fruit_dl`, `fruit_c`. All as `[r, g, b, a]` after the user's overrides (`appearance.resolve`).
- **Fruits:** `cherry apricot quince lemon pear greengage melon juniper blueberry damson grape fig` (a family's id hashes into these), `olive` (quiet), `plum` (groups) (`theme.rs:36-40`). A face carries its resolved tint (8.3), so a shell needs fruits only for Settings.
- **Themes:** `paper`, `pewter`, `dusk`; `medium` and `dark` read as Pewter and Dusk (`theme.rs:368-377`).

### 14.2 Follow the system

New: `view.theme {name: "system"}` (the default for a new install) picks Paper in light mode and Dusk in dark, and follows the shell's `display.dark`, which the shell updates with `lifecycle.appearance {dark}` (a new method) when the system changes. Today egui's light and dark visuals are both overwritten so the system mode never swaps the palette (`theme.rs:563`).

**As built (N3 slice 3), the simpler half:** over the C ABI a document's scene and `theme.get` are in the theme its last `view.theme {name}` named (`paper`, `pewter`, `dusk`; `medium` and `dark` read as Pewter and Dusk; any other name is the default, Paper), `Document::scene_theme`. Before, the C ABI's core took the command and built every scene in Paper whatever it named. Only the faces whose computed colours change (a family's tint) come in the next delta; a token's new value the shell reads from `theme.get` again. The name is the document's, not kept between runs (no preferences file is written), and the egui editor's own theme is untouched. A native shell follows the system by sending `view.theme` with Paper or Dusk when its appearance changes (the Mac app does, from `viewDidChangeEffectiveAppearance`), then `theme.get`. `view.theme {name: "system"}`, `display.dark` and `lifecycle.appearance` are not built.

**As built (N4 slice 4): the theme is the core's.** A C ABI core keeps one look for all its documents (`plumcraft_editor_core::settings::Look`): the preferences (the theme chosen, the person's colours, fruits and pinned families, text size, the run's look, engine picking), kept in `dirs.prefs/prefs.json` (`PLUMCRAFT_EDITOR_PREFS` wins: every script, and the iPad app, point it at scratch or the container; no `dirs.prefs`: in memory), and the system's appearance (`display.dark`, `display.contrast`, `display.reduce_motion`, `display.reduce_transparency` at start, then `lifecycle.appearance`). `view.theme {name}` takes `system` or a built-in theme's name and is kept (an unknown name is refused); **`system`, the default when nothing is chosen, shows Paper in light mode and Dusk in dark**. Every document's scene and `theme.get` are in the look's tokens: the theme shown, the person's changes over it, then **Increase Contrast** (`contrast: "more"`: dim and faint text, separators, field borders, wires, the grid and default pins moved towards the text colour, so no contrast pair the Settings checks name gets lower) and **Reduce Transparency** (every token made opaque over the canvas). **Reduce Motion** is the shell's: the Apple canvas holds a running face's wash instead of pulsing it. `theme.get` adds `chosen`, `system` and `gen`. A change to the look tells every document's shell (the feed's `prefs` domain, `{doc, domain: "prefs", gen}`), which reads `theme.get` and the settings again; the scene follows at its next look (faces whose computed colours changed). The Apple apps send `lifecycle.appearance` from their canvas when the effective appearance or the accessibility display options change, never `view.theme`. The egui editor keeps its own look and window; it takes `view.theme {name: "system"}` too (egui's `system_theme`) and refuses an unknown name. **Since egui's parity work (after N4, the audit's D6)** its Settings window has a Match the System box over the theme cards (`view.theme {name: "system"}`; unticked, the theme shown is kept as the choice), and an egui editor with no theme in its preferences follows the system as the core's look does, writing nothing until the person chooses; a theme already stored stays. egui reads the system's light or dark mode itself each frame (`system_theme`), not through `lifecycle.appearance`, and has no Increase Contrast or Reduce Transparency.

### 14.4 Settings as data (N4 slice 4)

`settings.get {fromDisk?}` → `{chosen, shown, system, gen, sections: [{id, label, note?, items: [item]}]}`, the egui Settings window's settings as a model a shell renders natively (`plumcraft_editor_core::settings::model`). An item is `{id, label, kind, value, command, param?, params?, group?, note?, …}`; a shell sets it with `command.run {command, params: params + {param: value}}`, so every setting goes through a command (golden rule 4) and nothing lives only in a shell's defaults. Kinds: `choice` (`options: [{value, label}]`), `number` (`min`, `max`, `step`, `unit`, `default`), `toggle`, `color` (`#rrggbb`, `default`, `changed`, `contrast: {ratio, with, need, level: ok | low | fail, text}`, `reset: {command, params}`), `action` (a button; `invoke: true` runs it as `ui.invoke` does, asking for a file), `note` (text alone, the system's accessibility settings). Sections: `appearance` (Theme: Match the System, Paper, Pewter, Dusk; Text size; Zoom on a shell that scales its window, egui only; the accessibility notes), `colours` (every Settings token of the theme shown, by its group), `families` (the fourteen fruits, and each family of the catalogue pinned to a fruit or its own), `files` (Import Theme…, Export Theme…, Reset), `running` (Colour Nodes While Running, Animation speed), `engines` (Pick Engines Automatically), `cache` (Open Cache Report). `fromDisk: true` builds the model from the preferences file as it is now, with `persisted` (what a reopen reads). The settings commands (`view.theme`, `view.textSize`, `view.zoom`, `theme.setToken`, `theme.setFruit`, `theme.pinFamily`, `theme.reset`, `theme.import`, `theme.export`) are carried out in the core for a document with a look, replying as the egui window does; `view.runColors`, `view.animationSpeed` and `engine.auto` are kept in the preferences and taken by every document. **Not honoured in the Apple apps:** Zoom (not offered: the system scales the display and the canvas zooms itself) and Text size, which changes the core's text sizes but not yet the Apple canvas's fonts (they follow Dynamic Type and the system size).

### 14.3 Text

Five styles today, scaled from a 14 pt base by `view.textSize` (11 to 18, `theme.rs:31`, `:541-561`): title 14 SemiBold, port 13, caption 12.5, small 12, tag 12 monospace. Inter is the face (OFL), Inter SemiBold for titles when installed (`fonts.rs`). A native shell uses the system face at the system's size (`display.text_size`) scaled by `view.textSize / 14`; `view.zoom` (0.8 to 1.6) scales the whole window on desktops, and is ignored where the platform scales text itself (Dynamic Type) (open question 10). The theme's `text` gives the sizes the core's own measurements assume.

## 15. Logging and diagnostics

**Today** there is no logging framework: diagnostics are `eprintln!`, the status bar's messages, an engine's `stderr_tail` and the session's journal (plan 2.1).

- **One facade** in the core, with levels `error warn info debug trace` and a target (`core`, `session`, `engine.<id>`, `store`, `ffi`, `shell`). Every `eprintln!` in the crates that move into the core becomes a log call; stderr stays a sink on desktops, so nothing printed today stops being printed.
- **Sinks:** stderr on desktops; the unified log on Apple and logcat on Android (the shell installs them; the core calls a `log` callback given in the config, or the shell reads `log.tail`); a bounded file `dirs.logs/plumcraft.log` (4 MiB, two kept); a ring of 10,000 lines read by `log.tail`.
- **Shell lines:** a shell logs its own events with `log.write {level, text}` (a new method) under target `shell`, so one tail shows both sides.
- **Status messages** stay what the user sees (14 in Appendix A); an error message is also logged at `warn` or `error`.
- **As built (N3 slice 5), the unified log on Apple:** the shell, not a config callback, reads the core's log. The kit's `UnifiedLog` (`PlumCraftShell`) follows the feed's `log {seq, worst}` mark, reads `log.tail {after}` and writes each entry to os_log under the app's subsystem with the entry's source as the category (`com.plumcraft.pad`, categories `ffi`, `core`, `session`, …); the shell's own lines go to os_log at once and to the core's log by `log.write`. `log stream --predicate 'subsystem == "com.plumcraft.pad"'` on the Mac (or `simctl spawn … log stream`, or Console for a device) shows both sides. The iPad app uses it; the Mac app keeps stderr.
- **Export Diagnostics** (a new command, `app.diagnostics`, File menu): a `.zip` in `dirs.logs` of the log file, the journal, each engine's `stderr_tail` and state, `ui.inspect`, the config without secrets; then the `share` ask. Never includes tokens or file contents.

## 16. Conformance

### 16.1 The suite

**As built (N2 slice 4).** A shell conforms when it passes every case of the suite. The cases are data, `crates/editor-core/fixtures/conformance/cases.json`; the runner is `plumcraft_editor_core::conformance`, written once against an abstract `Driver`:

```rust
pub trait Driver {
    fn name(&self) -> String;
    fn call(&mut self, method: &str, params: Value) -> Value;      // one control request: {ok, result | error}
    fn reports(&self) -> bool;                                     // does the suite give it its measurements?
    fn report_geometry(&mut self, faces: &Value) -> Result<(), String>;
    fn settle(&mut self) {}                                        // wait (on a condition) until it drew and measured
    fn hot_move(&mut self, at: [f32; 2], slop: f32) -> Option<Value> { None }  // plum_gesture_move, where it has one
}
```

- **A case** starts on a new graph (`graph.new {discard: true}`) and runs steps: `call` (a method, its params, `ok` wanted, `expect` (JSON pointers into the reply and what each must be or match), `save` (a value for later steps)), `gesture` (`ui.gesture`), `geometry` (report measurements, typed on the C ABI), `hot` (the typed drag move), `check` (the runner's own: every colour a token or a colour and `theme.get` has every token; no two nids alike). A point is `{"$locate": {…}}`, resolved through `geometry.locate`, so a case holds for whatever sizes the shell measured.
- **24 cases, 300 steps** since N3 slice 1 (19 and 197 at N2's end): hit testing on reported geometry; taps (select, add, take away, clear; watch a pin and a wire); a face dragged alone and with the selection, each one undo step; two drags as two steps and a held arrow key as one; a cancelled drag; a wire dragged, refused where it doesn't fit, picked up and rerouted or dropped; a new wire dropped on nothing opening the palette for what takes it, dismissed, and chosen from (one undo step, from an output and into an input); marquees, one-shot and dragged; double taps; context menus; a collapsed group's face; a frame's tab and handle; a bend point; the scene's colours and `theme.get`; nids; a run's state in the scene; watched text, scalar and picture values; and a hostile sweep of what a shell sends (`ui.gesture`, `geometry.*`, `scene.get`, `theme.get`, `palette.*`, `canvas.palette`'s wire, `edit.move`, `selection.nudge`), with the core still serving after.
- **Two halves (N3 slice 1).** Every case says its `half`. **A case is read-only (`read`) when nothing it asks of the shell after its setup changes the graph:** its setup builds the graph it reads with `command.run` (adding nodes, wiring, grouping, setting values), and after that it only hit-tests, locates, taps, double-taps and long-presses (selecting, watching, opening windows, menus and the palette), marquees, fits, gets the scene and the theme, runs and watches, and sends hostile input that is refused; `graph.get` is the same at its end as after its setup. **Every case that moves, wires, reroutes, routes, resizes, adds from the palette, nudges, undoes or redoes is an editing case (`edit`)**, and so is a cancelled drag (it is an editing gesture, even though it leaves nothing). 12 cases (141 steps) are read-only: a shell's measurements are what the core hits, a tap selects; Shift adds; primary takes away; empty canvas clears, a tap on a pin or a wire watches its value, a marquee selects what it touches, a double tap opens what it is on, a context gesture gives the menu of what it is on, the scene's colours are tokens alone or colours alone, and theme.get has every token, no two items of a scene share a nid, a run's state arrives in the scene, a watched text value arrives as text.plain, and a scalar as scalar, a watched picture arrives as picture, hostile gestures and queries are refused, and the core still serves. `conformance::run_half(driver, Half::Read)` runs them alone; N3's gate is that half on the Mac and iPad apps.
- **Needs (N3 slice 1).** A case may need `engine:text` or `engine:array` (an engine of that pack connected, for runs and watches) or `media` (the shell serves `media.get`); a driver says what it has (`Driver::has`), and a case it can't run is reported skipped, never passed. A step's `only` takes the same names. **Since N5 slice 4 nothing is skipped over a C ABI core's control port:** the port serves `media.get` (`docs/control-protocol.md`), a `hot` step runs as `geometry.dragMove` (the typed `plum_gesture_move`'s answer as JSON), and a driver over a running app's port that measures its own faces still runs the suite's `geometry` steps and the `only: "reports"` steps after them: it waits until the app has measured those faces, reports the suite's measurements over them (`geometry.set`) and waits until the core has them (the app reports again only when a face changes). The C# and Swift runners do this by default (`--own-geometry` keeps the old skip). The core's own document (no engine) skips the run and watch cases; the C ABI and the egui editor run the text cases on the text engine and the picture case on a second core or editor linked to the array engine.
- **Waiting (N3 slice 1).** A step with `until` sends its request again until the reply matches, settling the shell between (`Driver::pause`: a wake on the C ABI, a frame in egui), up to its `timeout`; runs are waited for with `ui.waitRun`. **Staying so (the N4 device gate, B43):** a step with `holds` (and `for`, seconds, default 1, at most 10) sends its request again, settling between, for that long, and every reply must match: for a state a shell could undo by itself a moment later (a window it closes in the core), which one reply can't see. New matchers: `$find` (an item of a list with given fields, and what it must hold) and `$min`.
- **Drivers today:** the C ABI headless, through the real exported functions (`plum_request`, `plum_poll` after a wake, `plum_geometry_set`, `plum_gesture_move`; `crates/ffi/tests/conformance.rs`); the C ABI's own control port, as JSON lines over TCP (same file); the egui editor through its control dispatcher with the window drawn between requests, measuring its own faces (`crates/ui/tests/conformance.rs`); and the core's own document (`conformance::tests`). All four pass every case.
- **Rows (N4 slice 2):** a step with `only: "rows"` runs on a driver whose rows' widgets and header parts are measured: one the suite gives its measurements to (`reports`), or a shell that reports them itself (the Apple apps' port driver says `has("rows")`). The egui editor doesn't: its own widgets take the clicks on its rows (and egui-snarl its drags), so a tap or scrub aimed at a row is the Apple shells' and the C ABI's; every driver runs the same edits through `row.edit` and `row.set`. Six cases came with this slice (30 in all): a value typed into a row (checked, held to its range, refused, one undo step, Tab, Escape), every widget kind set through its row, a switch's tap and a number's scrub (with the typed move's value), the more-button's menu, a wire running right to left routed and hit there, faces kept apart with the graph's positions unchanged.
- **N4 slice 3:** six cases (36 in all): the parameters inspector (every input listed, one hidden from the face, set while hidden, reset, shown again, closed), a group made, entered, edited inside, renamed in place, left, collapsed, ungrouped and undone, a frame made, renamed in place, resized and grown with a pushed member, a net made, joined, renamed in place and left, a pop-out opened by a double tap, brought forward, viewed and closed, and Select mode, Share and the toolbar as the core's.
- **Waiting:** a driver waits on conditions, never on fixed times: the egui driver draws until every face in the scene is measured (at most 5 s); the C ABI driver polls until `geometry.locate` reports the faces it measured.

**How a Swift or Kotlin shell runs it.** Its core opens a control port (config `control: {port: 0, token_file}`, 4.1); the shell's test harness starts the app with that port, and the runner drives it with the control-port driver (JSON lines, `auth` first: `crates/ffi/tests/conformance.rs`'s `Port`), from Rust (a small test binary pointed at the port) or from any language that reads `cases.json` and implements the step language above. **The Swift kit does (N3 slice 2):** `PlumCraftConformance` (`native/apple/PlumCraftFFI`) is the runner's step language in Swift, reading `cases.json` where it is, with two drivers: `KitDriver`, through the kit as an app drives it (requests through `PlumCore`, a `DocumentModel` following the feed, measurements typed through `plum_geometry_set`, the drag's move through `plum_gesture_move`), and `PortDriver`, the control-port driver over JSON lines with `auth` first. Both pass the read half on macOS (`swift test`); the picture case passes through the kit on a core opened on the array engine, and is skipped on the text engine. Requests then go through the shell's own core and its real geometry, since the shell reports what it measured (`reports() == false`); what the suite can't see (the shell's drawing, its gesture recognisers) is checked by the platform's own UI tests (plan 11).

**Against a running app (N3 slice 3).** The Mac app measures its own faces, so it is driven over its core's control port by `PortDriver(reports: false, doc:, engines:)`: the suite's `geometry` steps are skipped, every `settle` waits (on the condition, at most 10 s) until `geometry.locate` says each face in the scene is `measured`, each request names the app's document (`doc`), and `has("engine:…")` is the engines its core has. The runner is a command too, `plumcraft-conformance --port N --token-file F --cases cases.json --doc D --measured [--engine text] [--half read|edit|all] [--needing engine:array]` (`native/apple/PlumCraftFFI`); `native/apple/PlumCraftMac/conformance.sh` builds the app, opens an empty graph in it on scratch preferences and store with its port open, runs the half, and quits the app by its PID; then again on an app opened on the array engine for the cases that need it. The read half: 11 passed and 1 skipped on the text engine, the skipped picture case passing on the array engine. **A note for shells that report header `parts`:** the cases aim a tap at `{part: "header"}`, the header's centre; a shell whose reported `title` part covers that point turns the tap into a select and a watch (8.13), and "a tap selects" (which wants one command) fails. The Mac app reports no parts in this slice (its typed `plum_geometry_set` has none), as the egui shell reports none.

**Against the iPad app (N3 slice 5).** `native/apple/PlumCraftPad/conformance.sh [read|edit|all]` builds the app for the booted iPad simulator, installs it, opens an empty graph in it on scratch state with its port open (the simulator shares the Mac's loopback), checks that the app's store is inside its container, runs the half with the same runner and `--measured`, and terminates the app. Read half 12 of 12, edit half 12 of 12, no skips. `conformance.sh gestures` runs the app's UI test (XCUITest): a tap selects, a tap on empty canvas clears, a long press opens the core's context menu, a pinch zooms, a one-finger drag on empty canvas pans, a double tap on empty canvas fits, and the graph survives rotation, all through UIKit's recognisers, which the port can't reach. On a device the port is not reachable: the C ABI binds loopback only, and decision 10's debug binding to the local network is not built.

**N4 slice 4: 41 cases.** New: settings set, read back, kept (on a driver whose core has a preferences file, `prefs`: `settings.get {fromDisk: true}`) and reset; a theme chosen is the scene's, `system` shows Paper or Dusk, an unknown one is refused; autosave (on, a frame added and on disk once it settles, the file matching the graph; a file that can't be written reported `failed` with the dirty document kept; off); the control port asks for its token (`check: "authRefused"`, `only: "port"`: the Swift port driver opens a second connection with a wrong token and one that sends before authenticating, both refused; on a device this is the local network's port); a number's text (a FLOAT at its port's decimals, an INT whole) in `parameters.get` and the scene. Capabilities a driver may claim: `port` (over the control port), `prefs` (its core keeps a preferences file).

**B71 (2026-11-21): two cases, and the `manifests` capability.** "Install Engine… is offered as the platform allows, by the core" runs everywhere: `engine.list`'s `canInstall` and `installRefused`, `ui.inspect`'s `engine.canInstall` and `installRefused`, and the Engine menu's Install Engine… (`enabled`) agree, and Cancel Install is disabled with nothing installing. "An engine a manifest in the engines folder names is offered, and Reload Engine List keeps it" needs `manifests`: a driver whose shell's engines folder is the suite's, `crates/editor-core/fixtures/conformance/engines` (`probe.toml`, an engine running elsewhere, so every platform lists it and nothing starts it), through `Launcher::folder`, a config's `dirs.engines`, or `PLUMCRAFT_ENGINE_FOLDER` for an app (the Mac and iPad scripts set it, and pass the runner `--has manifests`).

**A copied manifest (2026-11-21): the `copied_manifest` capability, five steps and two matchers.** The manifests case has five more steps, each `only: "copied_manifest"`, so a driver without the capability (every native runner today) runs the case as before and skips them. A driver with it has as its engines folder the one `conformance::copied_manifest_folder(root)` makes in a scratch `root` of its own: `probe.toml` again, and `copied.toml`, a copy of a project's manifest (`uv run --project . python -m copied_engine`) with no project beside it and a stand-in `uv` that fails as Python would there (`docs/interface.md` 16.6). Its platform must start programs (`platform.subprocess`), or the copied engine is not offered. The steps: `engine.list`'s choice for `copied` has a `warning` and the probe's has none; `ui.inspect`'s `engine.choices` the same; the Engine menu has an item labelled "Copied Engine", enabled, with `warning` and a `hover` that starts with it, and the probe's has no `warning`. The matchers they use, for a runner that claims the capability: `$includes` (a string holding the given text) and `$deep` whose fields' wants may themselves be matchers (`{"$deep": {"label": "Copied Engine", "warning": {"$includes": "it may be a copy"}}}`), where a plain value is compared as before. The core's document and the C ABI headless claim it (`conformance::tests::the_copied_manifest_case_passes_on_the_cores_own_document`, `crates/ffi/tests/conformance.rs` `a_copied_manifest_is_marked_through_the_c_abi`).

**Still to come from Draft 1's list** (16.1 then; runs, `ui.waitRun` and watching the floor's three viewers came in N3 slice 1): widgets set by gesture on their rows, group and frame editing beyond moves, nets and slices, the other viewers and their tools, Save As and reopen through asks, every ask kind, engine switching, themes and text size, `lifecycle.*`. They are cases to add as each is built in the core; the hostile sweep grows with each new parser.

### 16.2 Shared fixtures

In `crates/editor-core/fixtures/`, decoded by Rust, Swift and Kotlin tests as `crates/proto/fixtures/` are today: `scene/full.json` (every row source, widget kind, footer kind and run state, wires plain, routed, sliced and through a net, a frame, a collapsed group, an open group, two faces measured, a fit asked for) and `scene/delta.json` (one node's run state changed), both built (N2 slice 2; `scene::tests::the_shared_fixtures_are_the_scene_and_decode` checks them and `PLUMCRAFT_WRITE_FIXTURES=1` rewrites them; since slice 4 a colour with a token carries the token alone); `conformance/cases.json` (16.1). Still to come: `menu.json`, `theme-*.json`, `asks.json`, `hostile/`. Schema: `crates/editor-core/schema/shell1.json` (the scene, its delta, `geometry.set`, `ui.gesture`, hits, wire targets, `geometry.locate`, `theme.get`).

## 17. Open questions

**Answered (the maintainer, 2026-11-19),** as pasted: "Shell interface Q: 1A; 2A; 3A; 4A; 5A; 6A; 7A; 8A; 9A; 10A; 11B; 12A; 13A; 14A; 15A; 16A; 17A; 18A; 19A; 20A \"Remember this a potentially useful option for later.\"; 21A". A is each question's recommendation below, so every one stands as recommended **except 11: documents share one in-process engine from the start** (B), not one engine per document. The note on 20 is about serving the control channel on the local network: kept as an option for later. 21 is not one of these (the 2 GB floor on mobile: `docs/research/24-native-shells.md` section 15.5). The sections above still say "proposed" where these questions touched them; they are brought into line as each stage builds them.

Each with the answer recommended.

1. **`ui.invoke` and blocking dialogs.** Today `ui.invoke graph.open` replies only when the native dialog closes (`docs/control-protocol.md`). With asks it can reply at once with the ask in `asks`, or wait until the ask is answered. *Recommended:* reply at once, with the ask; a script answers it with `ask.answer`. Under egui today a script could not answer the dialog anyway, so nothing a script relies on is lost.
2. **New canvas commands and `command.list`.** N1's gate is byte-for-byte replies; adding `selection.*`, `canvas.*`, `net.select`, `viewer.size` changes `command.list`. *Recommended:* register them in N2, behind `commands.canvas`, and update the probes' expectations then.
3. **Hooks have no document.** `Hooks` are `fn()` (`commands.rs:42-59`); with two documents an application's overlays can't tell them apart. *Recommended:* N1 adds a `DocId` argument; the research software's hooks are changed with it.
4. **Transient shell state in `ui.inspect`.** `canvas.editing`, `popups.anyPopup`, the rename window and the palette are UI state today. *Recommended:* the palette becomes core state (its index and search are logic, plan 4.1); rename is a `text` ask; `editing` and `anyPopup` come from `ui.report`. *The palette is core state since N3 slice 1* (12.7); the rest is not built.
5. **Viewer-local state.** Grid format, table sort, text wrap and find, PDF page and array log scale are egui-local and not commands (13.2). *Recommended:* make each a command so scripts and every shell share them; keep only per-frame state (mesh camera during a drag, pan) shell-local.
6. **Audio on mobile.** *Recommended:* the core keeps cpal (it supports CoreAudio on iOS and AAudio on Android); the shell owns the audio session category, interruptions and focus, and tells the core to stop on an interruption.
7. **Wire shape.** egui-snarl's wires are degree-5 Béziers; native toolkits draw cubics. *Recommended:* the core sends a cubic chain fitted to today's curve (two cubics per wire), and hit-tests that chain, so drawn and hit agree everywhere; egui draws the same chain from N2. *As built in N2 slice 3:* four cubics (8.8), since two strayed up to 2.2 graph units; egui-snarl still draws its own plain wires, up to 0.72 units off the chain (slice 4 put egui's faces where egui draws them, 8.11 `offset`, so its pins and the chains' ends now agree).
8. **Drop a wire on empty canvas.** It is not an egui behaviour today (egui-snarl drops it). *Recommended:* add it for every shell (plan 5.5 lists it for touch): the palette filtered to kinds that take the wire, one undo step for add and connect (a new command, `node.addWired`). *Built in N3 slice 1* (the maintainer's N2 exit decision 8; 12.7): `canvas.palette {wire}`, `palette.items`, `palette.choose`, `node.addWired`, on every shell; the egui shell originates it from egui-snarl's dropped-wire hook without forking it.
9. **Premultiplied alpha.** Today's pictures are straight alpha; Core Graphics draws premultiplied bitmap contexts and Android's `Bitmap` is premultiplied. *Recommended:* keep straight alpha in the contract (one format, what engines send); the Apple shell makes `CGImage`s with `alphaLast` (no copy) and the Android shell premultiplies in its one copy.
10. **`view.zoom` on mobile.** egui's zoom factor scales the whole window; iOS and Android scale text themselves. *Recommended:* honour `view.zoom` on desktops only; on mobile Settings hides it and the command is disabled with the reason.
11. **Two documents and one in-process engine.** Not known whether two sessions may share one in-process engine (plan 4.3). *Recommended:* one engine per document until the N1 audit says otherwise. **Decided otherwise: one in-process engine is shared by the documents from the start.** N1's audit then has to make that safe (both in-process engines report one session id today, `s-<pid>`), not decide whether to allow it.
12. **Node identity for hot calls.** *Recommended:* an interner per document mapping graph ids to `u64`, never reused, so a stale `nid` after a remove misses rather than hitting another node.
13. **Colours in the scene.** Tokens by name keep theme changes from rebuilding faces, but some colours are computed (family tints, wire types, run fills). *Recommended:* token names where the colour is a token, `[r, g, b, a]` where computed; a theme change bumps the `scene` generation only for faces whose computed colours change. *As built in N2 slice 2, not yet as decided:* every colour carried its `[r, g, b, a]` and, where it had one, its token. **Built as decided in N2 slice 4:** a token alone, or a colour alone (8, "As built"); `theme.get` has the three names the Settings table lacks. The full scene shrank by about 11% (the shared fixture: 120 KB to 107 KB).
14. **Text the core writes.** Status texts ("stale", "12.3 ms", "3 more fields"), menu labels and errors are English and made by the core. *Recommended:* keep them in the core (one wording everywhere) and localise there later; a shell never builds sentences.
15. **Run colours without per-frame scene changes.** *Recommended:* as section 8.6: the core publishes states and times, the shell animates with the formula given; the core checks `runAnim.strength` against the same formula in `ui.inspect`.
16. **Command count.** The plan says "the 111 or so commands"; 111 ids are written inline and the settings and named-port commands are built by helpers (`settings_cmds.rs:151-183`, `named_cmds.rs:258`), so there are more. *Recommended:* generate the menu fixture from the registry, never count by hand.
17. **The ⌘-drag reroute.** egui-snarl's ⌘-drag picks up a wire's end (`edit_view.rs:172`); its touch meaning is unsettled. *Recommended:* a drag starting on a wired input pin picks the wire's end up on every platform (iPad has no ⌘), and a drag on an output pin always starts a new wire. *Built in N2 slice 4* as `ui.gesture`'s drag (8.13); the egui shell keeps snarl's ⌘-drag, since snarl owns its wire drags.
18. **Hover-only information** (8.10) on touch. *Recommended:* the plan's popover for ports and wires; tooltips become accessibility hints and long-press menu subtitles. Where today's behaviour is only a tooltip (hash source, a contrast check), nothing native replaces it on touch, and the text moves to Settings or About.
19. **`open.external` on desktops.** The session spawns `/usr/bin/open` itself today. *Recommended:* always go through the ask, so a sandboxed Mac build (a later App Store build) works; the egui shell answers it by spawning as now.
20. **The control channel on a device.** Decision 10 allows binding to the local network in debug builds. *Recommended:* the config's `control.bind: "loopback" | "lan"`, `lan` refused in release builds by the core itself, not only by the shell.

## Appendix A. What `crates/ui` holds today, and where it goes

Every piece of UI-held state or behaviour found in `crates/ui/src`, and the section that carries it.

| File | What | Section |
|---|---|---|
| `lib.rs` | `FileDialogs` (9 methods) and `NativeDialogs` (rfd) | 11.4 |
| `lib.rs` | `Prompt` (`Discard {New, Open, Quit}`, `Review`, `Uploads`), `PromptAnswer`, prompt modals and texts | 11.2, 11.4 |
| `lib.rs` | `invoke`: forms, prompts, dialogs, selection fill-ins per command | 11.4 |
| `lib.rs` | `try_run` follow-ups: last error, Convert offer, fit and clear on new/open, rerun on watch, notes for bundle, expand, insert, paste | 6.2, 7.2 `status` |
| `lib.rs` | `run_all`: Open with Choose an App, Save Result ask | 11.4 |
| `lib.rs` | `ask_dialog` and remembered folders per purpose | 11.3 |
| `lib.rs` | `pump_runs`, `drain_engine`, `pump_saves`: outboxes, uploads prompt, saves on a thread, reruns of reaped values, doc-open and doc-edit polling | 5.4, 11.2 |
| `lib.rs` | `pump_run_anim`, `pump_theme`, `save_auto_engines`, `save_run_anim` | 8.6, 14 |
| `lib.rs` | `shortcuts`: ordering, disabled consuming, text-field deferral | 12.2 |
| `lib.rs` | `menu_bar`: registry menus, Engine choices, Animation Speed, Fit, Node › Add, Search… | 12.1, 12.3 |
| `lib.rs` | `banner_text`, `engine_label`, `engine_short_label`, `engine_status`, `window_title`, `document_name` | 7.2 `engine`, `document` |
| `lib.rs` | `status_bar`: engine cell, document cell, hash label, run cell with Cancel, cache line, Auto-run, save in progress, Convert offer, messages | 7.2 `status` |
| `lib.rs` | `status_messages` order and wording | 7.2 `status` |
| `lib.rs` | `watched_panel`, `read_app_hooks`, `panel_display` (application value over the run's) | 7.2 `watched`, 13 |
| `lib.rs` | `browse_for_path` (path widgets' `…`, relative paths) | 11.4 |
| `lib.rs` | `handle_close`, `quit`, `closing`, `close_next` | 4.3 |
| `lib.rs` | `--screenshot` (`Shot`), `render_offscreen` | 6.2, 11.2 |
| `lib.rs` | `copy_out`, `copied` | 11.4 |
| `lib.rs` | `checked`, `command_menu`, `menu_item`, `parse_shortcut` | 12.1, 12.2 |
| `lib.rs` | `no_window_tabbing` (macOS window tabbing off) | Not covered: a native Mac shell decides its own tabbing (plan 5.4 wants window tabs); the egui shell keeps it |
| `lib.rs` | `run_anim_plan` (upstream nodes and engine lanes) | 8.6 |
| `canvas.rs` | Snarl sync from the graph, `CanvasNode`, `InputRow`, `RowKind`, `input_rows` | 8.3, 8.4 |
| `canvas.rs` | Constants: caption, pin label and socket label widths, wire hit, port hit, wire width, status slot | 8.1, 8.4 |
| `canvas.rs` | `typed_label`, `typed_tip`, list marks | 8.4 |
| `canvas.rs` | `node_rects`, `to_global`, `set_transform`, fit frames and tries, `fit_transform` | 8.9, 8.11 |
| `canvas.rs` | `measured_for_tidy`, `over_node`, `wire_at`, `port_at`, `pin_screen`, `wire_midpoints` | 8.11, 9.2 |
| `canvas.rs` | Keys Tab, Space, F | 12.3 |
| `canvas.rs` | Reveal and highlight for 2 s | 8.3, 8.9 |
| `canvas.rs` | Double-click node → parameters; collapse chevron → `node.view` | 8.13, 8.3 |
| `canvas.rs` | Wire menu (`WireMenu`) and its items | 12.4 |
| `canvas.rs` | `hover_wire` (port and wire under the pointer) | 8.10 |
| `canvas.rs` | Clicks on pins and wires watch; double-click a wire pops out; right-click on canvas opens the palette | 8.13 |
| `canvas.rs` | `NodeViewer`: engine of a node, pins, named tag, input menu, `shown`, `run_fill`, faded thumbs, border, header, footer, convert path, own editor, save buttons, excerpt, engine text, app text, node menu, expand items, thumbnails and waveforms, hover card | 8.3–8.7, 12.4 |
| `widgets.rs` | `EditBuffers` (uncommitted values, browse request), `is_editing` | 8.5, 6.3 `ui.report` |
| `widgets.rs` | Number, integer, bool, text, multiline, path, colour, choice widgets; decimals rules; blank and clear; disabled choices; `inactive_note`; slider | 8.5 |
| `param_widgets.rs` | Help icon, toggle, segmented, select, multichoice chips | 8.5 |
| `params_view.rs` | The parameters sheet: rows, value cell (wired chip, bundle chip, editor), reset ↺, facts line, help text, fed choices, field rects | 8.4, 8.5, 12.5 |
| `edit_view.rs` | `Picked` selection, `absorb` of snarl's marquee, click rules | 8.13, 12.3 |
| `edit_view.rs` | ⌘-drag reroute reconcile | 8.13, open question 17 |
| `edit_view.rs` | `drag_moves` (selection and group moves) | 8.13 |
| `edit_view.rs` | Selection outline painting | 7.2 `selection` |
| `edit_view.rs` | `selection_sizes`, `select_all`, `select_result` | 8.11, 12.3 |
| `edit_view.rs` | Edit menu canvas items, `EditOp`, `ALIGN_EDGES` | 12.3 |
| `edit_view.rs` | Copy, cut, paste at the pointer, delete, ⌘A, Esc cancels a run | 11.4, 12.3 |
| `groups_view.rs` | Proxies (`Proxy`, `ProxyOut`, `proxy_node`), `Visibility`, pin-to-member mapping | 8.3, 8.4 |
| `groups_view.rs` | Node menu group items; proxy menu; group header actions; breadcrumb; group frames; `header_badges` | 8.3, 8.9, 12.4 |
| `groups_view.rs` | Rename window and the group parameter window (`group.editPromoted`) | 11.2 `text`, `form` |
| `groups_view.rs` | Focus (`enter_group`, `leave_group`, `set_focus`), `inspect` | 8.9, 7.2 `view` |
| `frame_view.rs` | Frame painting, tab, drag with what it held, resize handle, menu, swatches, `inspect` | 8.9, 12.4 |
| `net_view.rs` | Net tags, drivers, tag drags, drop onto pins, net menu, selected net, name editor (`NameEdit`, `NameFor`) | 8.8, 11.2 `text`, 12.4 |
| `wire_route.rs` | Routed wires, Catmull-Rom path, bend handles, drag, double-click removes, Add Bend Point, Straighten, hit tests | 8.8, 9.2 |
| `wire_view.rs` | Wire and port value previews: show after 250 ms, peek after 500 ms, cache of 512 displays, `inspect` | 8.10, 7.2 `watched` |
| `slice_view.rs` | Slice labels on wires, the slice editor | 8.8, 11.2 `text` |
| `bundle_view.rs` | Inherited and overridden sources, override badge (`bundle.reset`), Detach | 8.4, 12.4 |
| `run_view.rs` | `Thumbs` (texture caches, misses, 512 MB picture budget), `rgba_image`, `load_picture` (shrink to the GPU's limit) | 10.1, 10.2 |
| `run_view.rs` | Wire geometry (`wire_frame`, `wire_points`, `wire_distance`, `wire_midpoint`) | 8.8, 9.2, open question 7 |
| `run_view.rs` | Text formatters: `format_ms`, `cached_text`, `running_text`, `state_name`, `stale_tooltip`, `NOT_RUN`, `excerpt_lines`, `truncate_chars` | 8.3, 8.7 |
| `run_anim.rs` | `RunAnim`, `Shown`, strength, hold, fade, phase, replay, speeds | 8.6 |
| `app_hooks.rs` | Node overlays (text, value), wire marks, status segment and tone, repaint while shown | 8.7, 8.8, 7.2 `status` |
| `status_cell.rs` | Two-line status cells, engine status line, hash label | 7.2 `status`, 6.3 `core.hello` |
| `status_msgs.rs` | `Msg` (tone, text, detail), chips, `more…`, the Messages window | 7.2 `status`, 12.5 |
| `palette.rs` | `PaletteState` (query, selected, at), `PaletteIndex` search, `node_menus` (Add by category) | 8.9 `view.palette`, 12.3 |
| `viewers/mod.rs` | `Viewer` trait, `ViewInput`, registry and `register`, `kinds_for`, `viewable`, `aim`, `Pane` | 13 |
| `viewers/basic.rs` | Scalar, text (wrap, find), table (sort, colour), tree, items, hex | 13.1, 13.2 |
| `viewers/grid.rs` | Array values grid (format, colour, slice, range) | 13.1, 13.2 |
| `viewers/plot.rs` | Positions plot | 13.1 |
| `viewers/mesh.rs` | STL mesh, camera, shading | 13.1, 13.2 |
| `viewers/pdf.rs` | PDF pages (hayro), page stepper, zoom | 13.1, 13.2 |
| `viewers/wrap.rs` | Picture, SVG, document, array pictures, audio as viewers | 13.1 |
| `viewers/xml.rs` | XML outline | 13.1 |
| `viewers/yaml.rs` | JSON as YAML | 13.1 (moves to the core as is) |
| `media.rs` | Image decode with limits, SVG parse and raster, document view with Open in …, `ZoomPan`, `inspect` per media | 10.2, 13 |
| `array_view.rs` | Array pictures, views offered, zoom, log, stats, `inspect` | 13.2 |
| `picture_tools.rs` | Probe pin and hover, 1:1, detail zoom, block size, picture rects | 13.2 |
| `probe.rs` | Exact readings from the full array | 10.3, 13.2 |
| `compare_view.rs` | A and B sides, how, split, channel, difference result | 13.2 |
| `diff.rs` | The difference and its statistics | 13.2 (moves to `plumcraft-media`) |
| `watched.rs` | The Watched panel's read of the full value, `Fulls`, the panel layout, Opens (recent apps, notes), save buttons | 10.3, 13 |
| `popouts.rs` | Pop-out windows, `on_top` while an editor window has focus | 12.5, 7.2 `watched`. Floating above the editor is a desktop window behaviour; not covered on iOS and Android, where a pop-out is a scene or pane |
| `audio.rs` | Waveform thumbnails, the audio view, spectrogram computation, `inspect` | 8.7, 13.1, 13.4 |
| `playback.rs` | `Player` (cpal), Play, Stop, Loop, position | 13.4 |
| `value_editors.rs` | Application value editors (`EditorKind`, registry), windows, `inspect` | 8.5 `editor`, 12.5. Their drawing is re-written per shell (plan 7) |
| `rename_view.rs` | Rename window (node, frame) | 11.2 `text` |
| `form_view.rs` | Command forms | 11.2 `form` |
| `engine_view.rs` | Engine menu choices, Connector, Attach and Install forms, `pump_engine` (since B71 the Reloader, the Installer and following an install are the core's: `Launcher::reloader`, `install::Setup`, `Document::offer_installs`, `control::step`) | 12.1, 11.4, 7.2 `engine` |
| `settings_view.rs` | Settings window, `window_command` (appearance asks from `Views::ui_asks`), `settings_state` | 14, 12.5 |
| `about_view.rs` | About window | 12.5 |
| `cache_view.rs` | Cache status line and the Cache Report | 7.2 `status`, 12.5 |
| `graph_params_view.rs` | The graph's Parameters window: named ports, reorder, rename, jump to node | 12.5, 12.3 `canvas.reveal` |
| `recipe_view.rs` | What made the value (`viewer.recipe`) | 13.2 |
| `prefs.rs` | `Prefs` (theme, appearance, dirs, auto engines, run colours, speed, extra), `Store`, `dir_key` | 11.3, 14 |
| `appearance.rs` | Text size, zoom, overrides, fruits, pinned families, `resolve` | 14 |
| `theme.rs` | `Tokens`, three themes, fruits, family tints, state colours, `Text` sizes, contrast checks, `mix` | 14, 8.6 |
| `fonts.rs` | Inter and Inter SemiBold | 14.3. Not covered for native shells: they use the system face (open question 10's neighbour; plan 5.5 "Dynamic Type") |
| `icons.rs`, `icon_data.rs` | Lucide icons tinted per use | 8.3 (`icon` by name); shells ship the same set |
| `control.rs` | Control handlers, `inspect`, `ui.set`, `ui.audio`, waits, screenshots | 5.3, 6.2 |
| `offscreen.rs` | Offscreen render through egui_kittest | 6.2: egui shell only. Not covered for native shells, which render their own views offscreen |
| `link.rs` | The event pump thread, repaint wake | 5.1, 5.4 (`Waker`) |
| `craft_ui/` | Copied icon and theme helpers from craft-libs | egui shell only |
