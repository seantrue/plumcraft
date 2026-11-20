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
| `app` | The application's identity: the window title's name (today `EditorApp::app_title`, `crates/ui/src/lib.rs:499`), About, the prefs file's name, log lines. An application of the editor (PlumImpress) gives its own |
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

### 4.2 Platform capabilities

| Capability | When false |
|---|---|
| `subprocess` | Process, Python and HTTP-spawned engines are listed in the Engine menu and disabled with "this device can't start programs". `engine.attach` (a remote engine) stays |
| `craft_apps` | `doc.open` and Open in … are disabled with the reason; a craft document shows its rendition |
| `python` | Python engine modes are disabled with the reason |
| `multi_window` | `view.popout` opens the pop-out as a sheet or pane of the same window; at most one document is open |
| `pointer` | No hover: section 8.10 lists what hover does today and what replaces it |
| `keyboard` | Shortcuts are still in the menu model; the shell may not show them |
| `hover` | As `pointer`, for a stylus or trackpad that hovers without a pointer |
| `pencil` | Reserved for PlumImpress's mask painter (plan 5.5) |
| `max_picture_px` | Sent to engines as a hint (a run's `limits`), and caps `px` (section 10.4). Replaces the spike's 1280 px cap |

The mechanism is today's: a command's `enabled` returns `Err(reason)` (`crates/session/src/commands.rs:11`), shown as the disabled item's tooltip and in `command.list`'s `disabledReason`.

**As built (N1 slice 2, 2026-11-19).** `plumcraft_editor_core::engines::Capabilities` holds three of these so far: `subprocess`, `craft_apps`, `python`. A desktop's default has all three; a config read through the C ABI declares only what its `platform` object states, so one without `platform` declares none. `refuses(kind, mode)` gives the reason a hosting can't start, the most specific first: "‹Craft›: this device has no craft apps", "this device has no Python", "this device can't start programs". Without `subprocess` what is offered is text and array in-process, a running HTTP engine a manifest names, and `engine.attach`; an HTTP engine the editor would have to start is refused, like a process engine. `offers(caps)` lists every hosting with its reason. The C ABI's config reads `engines.default`, `engines.mode` and `platform`.

### 4.3 Core and documents

- **The core** is process-wide: preferences and the theme, the engine list and manifests, the store, the command, form, hook, viewer and value-editor registries, the log ring, the control channel.
- **A document** is one window (decision 8): its own `Session`, engine link, selection, view, scene, watched values, pop-outs, asks and generations. Requests that act on a document name it (`doc`, section 6.1).
- **Opening:** `document.open {path?, grant?, demo?}` → `{doc}`. Without `path`, an untitled document (today `graph.new`'s state, named `Untitled`, `crates/ui/src/lib.rs:540`). With `demo: true`, the engine's demo graph (today's no-argument launch).
- **Closing:** `document.close {doc, discard?}`. Over unsaved changes without `discard` the core raises the `prompt` ask `unsavedChanges` with `then: "close"` (today `Prompt::Discard { then: Quit }`, `crates/ui/src/lib.rs:1089`) and replies `{closing: false, ask}`; once nothing is unsaved it replies `{closing: true}` and the document's state goes. The shell closes the window after `closing: true`, or after the feed's `document` domain says `closing`.
- **Quitting:** `app.quit` (a command) closes every document in turn, each with its own prompt.
- **Process-wide state today.** These are `static` and process-global, which two documents must share safely or stop sharing (the N1 audit, plan 4.3): the command registry and app ids (`crates/session/src/commands.rs:256-260`), hooks (`:138`), forms (`:230`), the viewer registry (`crates/ui/src/viewers/mod.rs:113`), value editors (`crates/ui/src/value_editors.rs`), openers (`plumcraft_session::file_cmds::openers`), the hash algorithm (`plumcraft_proto::handle::Algo`), the spike's Send To targets. Hooks are `fn()` with no document argument, so an application's overlays cannot tell two documents apart (open question 3).

### 4.4 Life cycle and memory

| Method | Params | Core does |
|---|---|---|
| `lifecycle.background` | `{}` | Writes the journal and each dirty document's recovery copy; on a platform without background running (iOS), asks for background time and cancels runs still in flight when it ends (`run.cancel` per document). Replies when that is done |
| `lifecycle.foreground` | `{}` | Reconnects engines that went away while in the background; reruns a watched value whose bytes were reaped |
| `lifecycle.memory` | `{level: "low" \| "critical"}` | `low`: drops decoded rasters and the `Fulls` cache (`crates/ui/src/watched.rs:42`) not in use. `critical`: also drops thumbnails and every borrowed-but-released byte buffer, and lowers `px` to `DEFAULT_PX` (1024, `crates/session/src/viewer_cmds.rs:29`) |

None of these exist today. Autosave on iOS and Android (decision 7) is a document option, `document.open {autosave: true}`: the core saves to the document's file once an undo step has settled for 1 s and no run is starting; desktop keeps explicit Save.

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

`code` is new and additive: `bad_json`, `bad_params`, `unknown_method`, `unknown_doc`, `disabled`, `busy`, `timeout`, `not_allowed`, `internal`. `error` stays the human sentence it is today. Every request gets exactly one reply.

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

Each lists params, result and errors beyond `bad_params` (unknown fields are an error, as `ui.set`'s are today).

**`core.hello`** `{}` → `{shell: 1, caps: [..], app, core: {version, build}, hash: {algo, source}}`. The hash is the process's one algorithm (`docs/interface.md` 4.3), which the status bar shows today.

**`document.open`** `{path?, grant?, demo?, autosave?}` → `{doc, title}`. Errors: the open's own (`graph.open`'s). **`document.close`** `{doc, discard?}` → `{closing, ask?}`. **`document.focus`** `{doc}` → `{}`: the shell's frontmost document, for the control channel and the menu bar. **`document.list`** `{}` → `{docs: [{doc, title, path, dirty, focused}]}`.

**`state.get`** `{domains: ["document", "run", …]}` → `{<domain>: {gen, …}}`: the current state of each domain asked for (7.2). Unknown domains are skipped, not an error.

**`scene.get`** `{since?: gen}` → section 8.12. Built in N2 slice 2 (`plumcraft_editor_core::scene`), served by the one dispatcher on the egui editor's control port, the C ABI and the C ABI's own control port. `since` is a whole number (a scene `gen`); anything else, or another field, is refused with the reason. The first `scene.get` also starts the feed's scene changes for that document (7.1). **`menu.get`** `{context?: {kind, …}}` → section 12. **`theme.get`** `{}` → section 14.

**`geometry.set`** `{faces: [{node, rev, size, pins, rows?, header?}]}` → `{applied, stale}`. What the shell measured (8.11). `applied` counts faces taken; `stale` lists nodes whose `rev` is older than the core's (ignored; the shell measures again). Not journaled, no undo step. **As built (N2 slice 3):** a canvas command (`command.list`, `command.run`); `rev` optional; `pins` is `{in?: [y | null], out?: [y | null]}` in row order; also `parts`; the reply adds `unknown` (faces the graph doesn't have, skipped); **a stale face is kept all the same**, since an older measurement beats the default and a face measured during a stream of run changes would otherwise never land, and listed so the shell measures again. **`geometry.hit`**, **`geometry.wireTarget`**, **`geometry.wirePaths`**: section 9's hot calls as JSON (`docs/control-protocol.md`).

**`ui.viewport`** `{zoom, pan: [x, y], rect: [x0, y0, x1, y1], settled}` → `{}`. The canvas transform (`screen = graph · zoom + pan`, as `ui.set` defines it) and its rect in window points, reported when a gesture ends and at most four times a second during one (plan 5.3). `settled: true` at a gesture's end. `ui.inspect`'s `canvas` reads it.

**`ui.report`** `{palette?, rename?, editing?, popups?, windows?}` → `{}`. Transient state only the shell holds, for `ui.inspect` (open question 4): `editing` is `canvas.editing` (a field has focus or a drag is in progress, today `EditBuffers::is_editing`, `crates/ui/src/widgets.rs:42`); `popups` is `anyPopup`; `windows` the rects of the shell's windows.

**`ui.gesture`** `{kind: "tap" | "doubleTap" | "context" | "longPress" | "drag" | "marquee", phase?: "begin" | "move" | "end" | "cancel", at: [x, y], to?: [x, y], mods?: ["shift" | "primary" | "alt" | "ctrl"], slop?}` → `{commands: [{command, params, ok, result | error}], hit?, drag?, update?, context?, menu?, cancelled?}`. *As built (N2 slice 4), from every shell and the control channel:* a gesture the shell recognised, in graph units; the core publishes the scene, hit-tests it with `slop` (graph units, default 12: the shell's target size over its zoom) and runs what it means (8.13). `context` is a secondary click or a long press (`longPress` is the same); `drag` has a `phase`; `marquee` is a rect in one go (`at` to `to`), what a Select mode or a Pencil drag sends. Pan and zoom (`pinch`, `pan`) are not gestures here: they never cross (plan 5.3). Draft 1's `space: "screen"` and `scale` are not built: a shell converts. An unknown kind, phase, modifier or field, a point that isn't two finite numbers within ±1e7, and a `move` or `end` with no drag under way are errors. With no geometry reported the core hits its own estimates (220 × 160 faces), not an error.

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

**As built (C ABI, N1 and N2 slice 2):** `changed` carries `{doc, domain: "engine", gen, connected, label, ready}`, `{doc, domain: "document", closing: true}`, `{doc, domain: "menus", gen}` once the shell has called `menu.get`, and `{doc, domain: "scene", gen}` once it has called `scene.get`. The core looks at a document's scene when it is idle and at most every 16 ms while busy (a constant, `SCENE_EVERY`, not yet `display.frame_ms`), and a scene change not yet polled is replaced by the newer one, so a poll holds at most one scene change per document however fast a run reports. Since N2 slice 3 a look put off because the last was under 16 ms ago is taken when the 16 ms are up, not at the next 50 ms tick, so an edit or a geometry report reaches the feed and the hot calls' snapshot within 16 ms of the core going quiet. The egui editor's control port has no feed (it is request and reply); a script there calls `scene.get {since}` when it likes.

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
| `windows` | Which windows the document has open: parameters, value editors, settings tab, about, messages, cache report, graph parameters | `state.get` | `parameters`, `editors`, `about` |
| `prefs` | Theme, tokens, appearance, text size, zoom, remembered folders | `theme.get`, `state.get` | `theme`, `prefs` |
| `status` | The status bar's cells and messages (14 in Appendix A) | `state.get` | `lastError`, parts of `run` |
| `asks` | Pending asks (also pushed in `asks`) | `state.get` | `prompt`, `rename` |
| `log` | Lines in the ring | `log.tail` | none |
| `pictures` | Derived picture refs that became ready (10.2) | `state.get` | none |

A domain's JSON is, field for field, what `ui.inspect` reports for it today where it reports it; `state.get` adds `gen`.

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
- **An input row** is `{key, node, label, list, ty, pin: {shape, fill, wire, wireable}, source, text?, hover?, dim, from?, named?: {name, paint, hover}, badge?, widget?, inactive?}`: `node` is the member a proxy's row stands for (the draft's `member`); `from` is `inherited`'s "from <bundle>". An output row is `{key, node, label, list, ty, pin, hover}`; a tap watches `{node, port: key}`.
- **A widget** is `{kind, value, default, blank, clearable, min, max, softMin, softMax, step, decimals, int, choices: [{value, label, disabled?}], pathMode?, colorFormat?, hover?}`.
- **A preview** is `{kind: "rgba8", handle, w, h, size, hoverSize, items, summary, note?, faded}` or `{kind: "peaks", handle, channels, size, note?, faded}`; `handle` is the value's `DataRef` as JSON (what `media.get` takes). A text excerpt is a footer item, not a preview.
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
| `socket` | A socket-only input: `text` is the label with its type (`audio · WAV`) unless the label names it (`typed_label`) |
| `inherited` | From the bundle wired into the node: label in italics, the bundle's value, "from <word>" |
| `own` | The widget, with the caption dim when the value is the default |
| `readonly` | A widget the editor doesn't know: `label: value`, or an application editor's summary |

- `pinShape`: `circle` for scalars, `square` otherwise; `pinColor` the wire type's colour, else the `pin_default` token; a non-wireable port has `wireable: false` and a hidden pin (`canvas.rs:1045`, `:1230`).
- `list: true` adds `[ ]` after the label (inputs) or before it (outputs).
- `named`: the net-label tag `name →` before the caption, for a named port (`canvas.rs:1053`).
- `inactive`: "Not used now: read only when …" (`widgets::inactive_note`), shown in the parameters sheet.
- `member`: on a proxy's row, the member's `{node, port}` that the row stands for.
- Variadic slots and a choice's sub-ports are rows of their own (`input_rows`, `canvas.rs:150`): `key` is `c2` or `mode.turn`.

An **output row**: `{key, pin, label, list, ty, pinShape, pinColor, hover, tap: {command: "run.watch", …}, menu: "output", member?}`.

### 8.5 Widgets

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

### 8.7 Previews and footers

`preview` (only in view `full`, `canvas.rs:1629`):

```json
{"kind": "rgba8", "handle": "md5:nd1:…", "w": 640, "h": 480, "size": [80, 60], "items": 12,
 "summary": "array HSI 64×80 float32", "note": "From the previous run: …", "faded": false, "missing": false,
 "tap": {"command": "run.watch", "params": {"node": "n2"}}, "hoverSize": [160, 120]}
```

`kind` `peaks` (a waveform, `{handle, channels}`; decoded with `media.peaks`, drawn in `waveform` on `waveform_bg`, `audio::THUMB_SIZE`). `missing`: the bytes were reaped; the shell shows "re-run to refresh" on `placeholder`. A shape the core can't read is `{"kind": "unreadable", "text": "preview unreadable: …"}`.

Footer items, in order (`canvas.rs:1267-1390`):

| `kind` | Fields | Action |
|---|---|---|
| `fields` | `text` ("3 more fields" or "Fewer fields"), `hover` | `node.fields {node, all}` |
| `convert` | `lines`, `warn` (a step isn't lossless) | none |
| `editor` | `label`, `hover` | `node.editor {node}` |
| `progress` | `fraction` 0..1, `stage` | none |
| `error` | `text` (cut to 180 chars), `full`, `token` (`text_dim` for skipped, `error` otherwise) | none |
| `save` | `buttons`: Save Result…, Contact Sheet… (when items > 1) | `run.save {node, port, sheet, ask: true}` (11.4) |
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
- `via`: bend points; a shell draws a handle on each (radius 5, `wire_route.rs:32`) for drag (`wire.route`), double-tap to remove one, and the menu's Add Bend Point and Straighten.
- `color`: the wire type's colour, else the `wire` token (`wire_route::wire_color`).
- `net`: the net's key when the wire is drawn through a net label; then `hidden: true` and the net carries its tags.

A **net** (`crates/ui/src/net_view.rs`): `{key, name, color, selected, driver: {pin, at: [dx, dy], text: "picture ×3"}, uses: [{to, pin, at: [dx, dy]}]}`, `at` the tag's offset from its pin (default `net_view::default_use_at`). Tags: tap selects the net (faint lines to every use), double-tap goes to the driver or the next use, drag moves (`wire.labelAt`, `net.move`), drop on an input wires it from the net (`net.connect`, ringed in `highlight` when it fits and `error` when not).

A **mark** (`Hooks::wire_marks`): `{to: {node, port}, label, tone: "info" | "warn" | "error", tooltip, at: [x, y]}`, drawn on its wire, outlined in the tone's token (`accent`, `warning`, `error`).

### 8.9 Frames, groups, focus, view

A **frame** (`crates/ui/src/frame_view.rs`): `{id, nid, rev, title, pos, size, color: "#rrggbb" | null (the accent), holds: {nodes, groups}, menu: "frame"}`. Drawn under the nodes; its tab (☰ menu, icon `frame`, title) sits outside its top edge; the corner handle resizes (`frame.resize`); dragging the title moves it with what it held when the drag began (`frame.move`). Swatches are `frame_view::SWATCHES`.

An **open group's frame**: `{id, name, rect, depth, menu: "group"}`, computed by the core from its members' and nested proxies' measured rects (so it needs geometry), deeper groups first (`groups_view.rs:771`). Its band has a collapse chevron (`group.collapse {collapsed: true}`), the group icon, the name (double-tap renames), enter (`canvas.focus`) and a menu; dragging the band is `group.move`.

**Focus** (the group entered, `canvas.focus`; the document's since N2 slice 1, `Document::focus`): `view.focus: {group, path: [{id, name}]}` for the breadcrumb ("Graph › outer › inner", each a `canvas.focus`; Leave is `canvas.focus {group: null}`). While focused only the group's members are in `nodes`.

**The view domain**: `{target: {zoom, pan, seq} | null, fit: {seq, rect} | null, reveal: {node, seq} | null, palette: {open, query, selected, at, results} | null}`. The core asks for a transform (after `graph.new`, `graph.open`, `graph.tidy`, `ui.set`, View › Fit, a reveal); the shell animates there and reports back with `ui.viewport`. `fit.rect` is every face's rect (and frames' outside a focused group) expanded by 32, zoom clamped to 0.2..1 (`canvas::fit_transform`, `canvas.rs:318`); the shell fits it to its own rect. **As built (N2 slice 3):** only `fit`, as the scene's top-level `view: {fit: {seq, rect}}`, set by `canvas.fit` (whose reply carries it too), computed from the core's geometry; `seq` rises with each request, so a shell fits once a request. **Since N2 slice 4** `graph.new` and `graph.open` owe a fit when they leave something to show: the `geometry.set` (or `plum_geometry_set`) that measures the last face shown asks for it, once, in `view.fit`. The egui shell measures in process without `geometry.set` and fits those itself, waiting for its sizes, as before.

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
- `parts` names the header's parts (`title`, `chevron`, `icon`, `menu`, `enter`, `preview`) and the footer's items, for the hit test's parts (9.2); unknown names are refused.
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
| Tap a face's body, header, title, row, preview or footer | `selection.set {nodes: [id]}` (a collapsed group's face: `{groups: [g]}`); Shift `mode: add`, `primary` `mode: remove`. A title, preview or footer item also `run.watch {node}` when the node has an output (only where the shell reported those `parts`) |
| Tap a header's chevron, enter button | `node.view` (full ↔ collapsed) or `group.collapse {collapsed: false}`; `canvas.focus {group}`. No selection change |
| Tap an output pin; a wired input pin | `run.watch {node, port}` of the output; of the wire's source. An unwired input: nothing (its row shows its value). No selection change |
| Tap a wire | `selection.clear` (unless Shift or `primary`), then `run.watch` of its source |
| Tap a net's tag | `net.select {name, group?}`, or `net.select {}` when that net is selected |
| Tap a frame's tab | Nothing (the tab's menu and title are the shell's controls) |
| Tap empty canvas, a bend point, a frame's body, a group's band | `selection.clear` (unless Shift or `primary`); on empty canvas also `net.select {}` when a net is selected |
| Double tap a face | `node.parameters {node}`; a collapsed group's: `canvas.focus {group}` |
| Double tap a wire; a bend point; a driver's tag; empty canvas | `view.popout {node, port}` of its source; `wire.route` without the point; `canvas.reveal` of the driver; `canvas.palette {open, at}` |
| Context (secondary click, Control-click, long press) | No command: `context` (what `menu.get` takes: `node`, `proxy`, `input`, `output`, `wire` (a bend point's too), `net`, `frame`, `group`) and `menu`, the menu model for it. On empty canvas: `canvas.palette {open, at}` |
| Drag a face | `begin`: the faces moving: the whole selection when the face is in it (selected groups not inside another selected one, and the selected nodes their groups don't carry), else that face alone, and the selection stays. `end`: `group.move` and `node.move` by the offset; nothing when it didn't move |
| Drag an output pin, or an unwired input pin | A new wire. `move`: the target (`geometry.wireTarget`'s, `fits`). `end` on a pin: `wire.connect {from, to}` (refused, with the reason and a Convert offer, where it doesn't fit); on nothing: nothing (question 8's palette is not built) |
| Drag a wired input pin | The wire's end picked up (question 17, every platform). `end` on another input: `wire.reroute {to, newTo}`; on its own input: nothing; on nothing: `wire.disconnect {to}` |
| Drag a bend point; a frame's tab; a frame's handle; an open group's band | `end`: `wire.route` with the point moved; `frame.move` (with what it holds); `frame.resize`; `group.move` |
| Drag empty canvas, a wire, a tag, a frame's body | A marquee (on touch the shell pans there instead, and sends nothing). `move`: its `rect` and the faces it `touches`. `end`: as a marquee |
| Marquee | `selection.set {nodes, groups}` of every face it touches: plain `replace`, Shift `add`, `primary` `remove` |

- **A cancelled drag** (`phase: cancel`) runs nothing; the document is as it was (the suite compares `graph.get`, `dirty` and the undo label before and after).
- **Undo:** a drag runs at most one command that is an undo step (`wire.connect`, `wire.reroute`, `wire.disconnect`, `wire.route`). Moves are not undo steps in this editor (`node.move`, `group.move`, `frame.move` and `frame.resize` never were), so a drag of faces adds none; whether a move should be one is an open decision (plan section 17, slice 4).
- **The egui shell** sends its clicks (a tap) and its Shift-drag marquees as `ui.gesture` (since slice 4; snarl's private selection is no longer read). egui-snarl owns node drags, wire drags and double and secondary clicks there, so those are still egui's own code (plan section 17, slice 4, lists them).

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

`plum_gesture_move(doc, x, y, slop) → plum_drag`: where the drag under way in document `doc` is with the pointer at `(x, y)`: `kind` (1 move, 2 wire, 3 marquee, 4 bend point, 5 frame, 6 frame's size, 7 open group), `serial` (as the drag's `begin` replied), the offset `dx, dy` from where it began, a marquee's `rect`, and a wire's `target` (a `plum_hit`: the pin it would land on and `fits`, as `plum_wire_target`). Nothing changes. It reads the document's drag (a cell the core writes when `ui.gesture` begins or ends one, before its reply) and its snapshot, from any thread, without waiting for the core. `PLUM_ERR_MISSING` when no drag is under way or no scene is published, `PLUM_ERR_ARG` for a point that isn't a number. A shell moving faces calls `plum_wire_paths` with the `faces` the `begin` named and the `dx, dy` this gives. Measured: see plan section 17, slice 4.

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
| `prompt` | `{kind, title, text, answers, detail?}` | `{answer}`, one of `answers` |
| `form` | `{command, title, intro, fields: [{key, label, hint, kind: "line" \| "secret" \| "text", required}], submit, values}` | `{values: {key: text}}`; the core runs the command (`CommandForm::params`, `commands.rs:201`) and, on an error, raises the form again with `error` |
| `text` | `{purpose, title, label, text, placeholder, hint?, validate?}` | `{text}` |
| `clipboard.set` | `{text, kind: "workflow" \| "text"}` | `{}` |
| `clipboard.get` | `{kinds}` | `{text}` or cancel |
| `share` | `{path, subject, target?}` | `{notice}` ("Copied", "Sent") or `{error}` |
| `open.external` | `{path?, url?, app?}` | `{}` or `{error}` |
| `screenshot` | `{path, offscreen}` | `{path, width, height, source}` |

`prompt.kind` and `detail`: `unsavedChanges` (`answers: [save, discard, cancel]`, `then: new | open | close | quit`); `reviewWorkflow` (`[trust, cancel]`, `detail`: the `trust::Review` of what the graph touches); `allowUploads` (`[trust, cancel]`, `detail: [{to, file}]`). These are today's (`crates/ui/src/lib.rs:321-373`), with today's texts.

`text.purpose`: `rename.node` (`node.title`), `rename.frame` (`frame.set {title}`), `rename.group` (`group.rename`), `rename.net` (`net.rename`), `name.port` (`graph.port.name`), `rename.port` (`graph.port.rename`), `label.uses` (Label All Uses…, `wire.label` on every wire from the output), `slice` (`wire.slice`, hint "start:stop:step or an index"). Today these are windows of the editor's own (`rename_view.rs`, `net_view::NameEdit`, `slice_view::SliceEdit`).

### 11.3 The file rule

- The core and engines only see ordinary paths inside the declared roots or returned by a file ask (plan section 6).
- **Grants** are opaque strings the shell keeps (an iOS security-scoped bookmark, an Android persisted URI permission). The core stores `{path, grant}` in recent files and in a document's record, and hands the grant back in `document.open` so the shell can reopen access first.
- **A save to a destination without a path** (Android SAF, an iOS export): the shell answers `file.save` with a path in `dirs.scratch` and `commit: true`; the core writes there, sends `committing {ask, path}`, the shell streams it to the destination and calls `file.commit {ask, ok, error?}`. The save is in progress until then.
- **An open without a path** (a content URI, a photo): the shell copies the file into `dirs.caches/imports/<digest>.<ext>`, named by its content so a second pick costs nothing, and answers with that path.
- `startIn` is the folder this purpose last used (`prefs.dirs`, keys in `prefs::keys`, `crates/ui/src/prefs.rs:31`), else the document's folder, else `dirs.documents`. The core remembers the folder of every answer, as `ask_dialog` does (`lib.rs:658`).

### 11.4 Today's dialogs and prompts, one for one

| Today | Ask |
|---|---|
| `FileDialogs::pick_path(hint, current)`, mode `open` or empty | `file.open {purpose: "path", kinds: hint.filters, startIn: the field's folder, else prefs.dirs[dir_key]}`; answer → `paths::chosen_path` (relative to the workflow's folder) → `node.set` (`lib.rs:2182-2207`) |
| `pick_path`, mode `save` | `file.save {purpose: "path", name: the field's file name}` |
| `pick_path`, mode `dir` or `folder` | `folder.pick {purpose: "path"}` |
| `pick_files(hint, current)` | `file.open {purpose: "files", multiple: true}`; answer → one path per line → `node.set` |
| `start_in(dir)` | `startIn` on every file ask (11.3) |
| `open_workflow()` | `file.open {purpose: "workflow", kinds: [{label: "Workflow or bundle, <openers>", extensions: ["json", "zip", …openers]}]}` (`lib.rs:231`) for `graph.open`, `graph.insert`, `graph.insertGroup` |
| `save_workflow(current)` | `file.save {purpose: "workflow", kinds: [{Workflow, [json]}], name: current or "untitled.json"}` for Save of an unsaved graph and Save As |
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
- An item: `command`, `params?` (an item that runs a command with fixed parameters), `label`, `shortcut?`, `enabled`, `reason?` (the disabled item's tooltip), `checked?` (today `run.auto` and `view.runColors`, `lib.rs:2242`), `hover?`, `form?: true` (it asks first: the shell adds `…` per platform style).
- Items that are not plain registry entries come from the core too (12.3): the Engine menu's choices, Node › Add by category and by engine, Animation Speed, the Edit menu's canvas items (commands since N2 slice 1, Align › and Distribute › excepted), View › Fit, Node › Search…
- Activating an item is `ui.invoke {command}` (or `command.run {command, params}` when it has `params`), never a shell-side interpretation.

### 12.2 Shortcuts

Today's notation is egui's: `Cmd+Shift+Z` (`CommandSpec::shortcut`, `commands.rs:19`; parsed by `parse_shortcut`, `lib.rs:2316`). The menu model gives `{mods, key}`:

| Token today | `mods` | macOS | iPadOS | Android, Linux, Windows |
|---|---|---|---|---|
| `Cmd` | `primary` | ⌘ | ⌘ (`UIKeyModifierFlags.command`) | Ctrl |
| `Ctrl` | `ctrl` | ⌃ | ⌃ | Ctrl |
| `Shift` | `shift` | ⇧ | ⇧ | Shift |
| `Alt`, `Opt` | `alt` | ⌥ | ⌥ | Alt |

`key` is egui's key name: letters, digits, `Enter`, `Period`, `Comma`, `Escape`, `Tab`, `Space`, `Delete`, `Backspace`, arrows. The registry's shortcuts today: `edit.undo` Cmd+Z, `edit.redo` Cmd+Shift+Z, `graph.new` Cmd+N, `graph.open` Cmd+O, `graph.save` Cmd+S, `graph.saveAs` Cmd+Shift+S, `app.quit` Cmd+Q, `app.settings` Cmd+Comma, `node.duplicate` Cmd+D, `group.create` Cmd+G, `group.ungroup` Cmd+Shift+G, `graph.tidy` Cmd+Shift+L, `run.start` Cmd+Enter, `run.cancel` Cmd+Period. Rules kept from today: the most specific shortcut matches first (`lib.rs:1559`); a disabled command still consumes its keys; `edit.*` shortcuts are left to a focused text field; a shortcut fired in a text field runs after the field commits (`lib.rs:1575`).

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
| Node › Search…, Tab, Space, right-click on canvas | `lib.rs:1644`, `canvas.rs:585`, `:873` | `canvas.palette {open?, at?, query?}`, `at` in graph coordinates (`ui.set {palette}` runs it) | shell (open question 4 makes the palette core state later) |
| Node › Add › (by category, by engine) | `palette::node_menus` | `node.add {kind, pos}` items in the menu model (since N1 slice 8) | core |
| Enter group, Leave, breadcrumb | `groups_view.rs:747-770` | `canvas.focus {group \| null}` (`ui.set {focus}` runs it) | core state; the shell rebuilds and fits |
| Reveal a node, a net tag's double-click | `canvas.rs:423`, `net_view.rs:324` | `canvas.reveal {node}` | shell |
| Esc cancels a run | `edit_view.rs:577` | `run.cancel` (already a command), bound to Escape by the shell while a run is in flight | core |
| Select Net, Go to Driver, next use | `net_view.rs:474-490` | `net.select {name?, group?}` (a tag's click toggles it, Esc and an empty click clear it); Go to Driver and Next Use stay shell items that resolve to `canvas.reveal` (next use cycles per tag, shell state) | core; reveal shell |

The keys (⌘A, Delete, Backspace, Escape, F, Tab, Space, ⌘X/C/V) are recognised by the shell and dispatched as these commands; they are not in `shortcut_table()` (12.2), which stays the registry's, because a shell's text fields and popups decide when the canvas has the keyboard. The menu model shows them on its items.

### 12.4 Context menus

`menu.get {context: {kind, …}}` returns a menu like 12.1's. The kinds and their items today:

| Kind | Items (command ids) | Source |
|---|---|---|
| `node` | Edit Parameters… (`node.parameters`); the application editor (`node.editor`); the id and kind (a label); Rename… (`text` `rename.node`); View Full, No preview, Collapsed (`node.view {nodes, view}`, the selection when the node is in it); Keep result When slow, Always, Never (`node.cache`); Run on <engine>, Any (`node.engine`); Watch <port> (`run.watch`); Save Result…, Contact Sheet… (`run.save {ask}`); Detach (`bundle.detach`); Expand into Group, Expand without a Group (`node.expand`); Open <port> in <App> (`doc.open`); Open <port>, Open <port> with › recent apps, Choose an App… (`value.open`); Duplicate (`node.duplicate`); Remove (`node.remove`); Group <selection>, Collapse into a group (`group.create`); Promote ›, Expose ›, export mark (`group.promote`, `group.unpromote`, `group.output`, `group.removeOutput`, `group.checkpoint`) | `canvas.rs:1472-1626`, `groups_view.rs:357-466` |
| `proxy` | Expand (`group.collapse`), Remove output (`group.removeOutput`), Watch <output> (`run.watch`), Rename… (`text` `rename.group`), Ungroup (`group.ungroup`) | `groups_view.rs:640-668` |
| `input` | Name This Port… (`text` `name.port`); or Rename Port… (`text` `rename.port`), Remove Name (`graph.port.unname`); a wired input says it can't be named | `canvas.rs:1062` |
| `output` | Watch <port> (`run.watch`) | `canvas.rs:1257` |
| `wire` | Watch (`run.watch`), Pop Out (`view.popout`), Slice… or Edit Slice… (`text` `slice`), Clear Slice (`wire.slice {slice: null}`), Add Bend Point (`wire.route {via}` with the point), Straighten (`wire.route {via: []}`), Label All Uses… (`text` `label.uses`), Disconnect (`wire.disconnect`) | `canvas.rs:821-862` |
| `net` | Go to Driver or next use (`canvas.reveal`), Watch, Pop Out, Select Net (`net.select`), Rename Net… (`text` `rename.net`), Draw as Wire, Draw All as Wires (`wire.unlabel`) | `net_view.rs:470-510` |
| `frame` | Rename… (`text` `rename.frame`), Colour › swatches and Theme accent (`frame.set {color}`), Remove Frame (`frame.remove`) | `frame_view.rs:167-192` |
| `group` | Collapse (`group.collapse`), Enter (`canvas.focus`), Rename… (`text` `rename.group`), Ungroup (`group.ungroup`), the group's parameters (`group.editPromoted`, `group.unpromote` as a `form`) | `groups_view.rs:440-640` |
| `canvas` | Today the palette, not a menu. Proposed for touch: Add Node… (palette), Paste, Select All, Fit | `canvas.rs:873` |

A context menu's `params` name what was hit, so a shell passes the hit through: `menu.get {context: {kind: "wire", to: {node, port}, at: [x, y]}}`.

### 12.5 Forms and windows

`CommandForm`s are asks (11.2). The document's other windows are state in the `windows` domain, each opened and closed by its command: the parameters sheet (`node.parameters`), value editors (`node.editor`), Settings (`app.settings {tab}`), About (`app.about`), Messages (`view.messages`), the Cache Report (`run.cacheReport`), the graph's Parameters (`view.params`), pop-outs (`view.popout`, `view.close`). The shell draws each natively from the domain and the core's data for it; their controls are commands.

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
| List item shown, array view (auto, heat, …) | `run.item`, `run.view` | Commands |
| Grid format, colour by value, slice, range | `viewers/grid.rs:335`, egui-local | Proposed commands `viewer.grid {format?, colored?, slice?}` (open question 5) |
| Table sort, colour | `viewers/basic.rs:311`, egui-local | `viewer.table {sort?, colored?}` |
| Text wrap, find | `viewers/basic.rs:153`, egui-local | `viewer.text {wrap?, find?}` |
| PDF page | `viewers/pdf.rs:149`, egui-local | `viewer.page {page}` |
| Mesh camera | `viewers/mesh.rs:341`, egui-local | Shell-local, reported in `ui.report` (a drag turns it per frame) |
| Array pictures zoom, pan, log scale | `array_view.rs:132`, egui-local | Zoom through `viewer.zoom`; pan shell-local; `viewer.array {log}` |

Every viewer's state appears in `watched.panelViewer.state` (and each pop-out's) as `ui.inspect` reports it today.

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

### 14.3 Text

Five styles today, scaled from a 14 pt base by `view.textSize` (11 to 18, `theme.rs:31`, `:541-561`): title 14 SemiBold, port 13, caption 12.5, small 12, tag 12 monospace. Inter is the face (OFL), Inter SemiBold for titles when installed (`fonts.rs`). A native shell uses the system face at the system's size (`display.text_size`) scaled by `view.textSize / 14`; `view.zoom` (0.8 to 1.6) scales the whole window on desktops, and is ignored where the platform scales text itself (Dynamic Type) (open question 10). The theme's `text` gives the sizes the core's own measurements assume.

## 15. Logging and diagnostics

**Today** there is no logging framework: diagnostics are `eprintln!`, the status bar's messages, an engine's `stderr_tail` and the session's journal (plan 2.1).

- **One facade** in the core, with levels `error warn info debug trace` and a target (`core`, `session`, `engine.<id>`, `store`, `ffi`, `shell`). Every `eprintln!` in the crates that move into the core becomes a log call; stderr stays a sink on desktops, so nothing printed today stops being printed.
- **Sinks:** stderr on desktops; the unified log on Apple and logcat on Android (the shell installs them; the core calls a `log` callback given in the config, or the shell reads `log.tail`); a bounded file `dirs.logs/plumcraft.log` (4 MiB, two kept); a ring of 10,000 lines read by `log.tail`.
- **Shell lines:** a shell logs its own events with `log.write {level, text}` (a new method) under target `shell`, so one tail shows both sides.
- **Status messages** stay what the user sees (14 in Appendix A); an error message is also logged at `warn` or `error`.
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
- **19 cases, 197 steps** today: hit testing on reported geometry; taps (select, add, take away, clear; watch a pin and a wire); a face dragged alone and with the selection; a cancelled drag; a wire dragged, refused where it doesn't fit, dropped on nothing, picked up and rerouted or dropped; marquees, one-shot and dragged; double taps; context menus; a collapsed group's face; a frame's tab and handle; a bend point; the scene's colours and `theme.get`; nids; and a hostile sweep of what a shell sends in N2 (`ui.gesture`, `geometry.*`, `scene.get`, `theme.get`), with the core still serving after.
- **Drivers today:** the C ABI headless, through the real exported functions (`plum_request`, `plum_poll` after a wake, `plum_geometry_set`, `plum_gesture_move`; `crates/ffi/tests/conformance.rs`); the C ABI's own control port, as JSON lines over TCP (same file); the egui editor through its control dispatcher with the window drawn between requests, measuring its own faces (`crates/ui/tests/conformance.rs`); and the core's own document (`conformance::tests`). All four pass every case.
- **Waiting:** a driver waits on conditions, never on fixed times: the egui driver draws until every face in the scene is measured (at most 5 s); the C ABI driver polls until `geometry.locate` reports the faces it measured.

**How a Swift or Kotlin shell runs it.** Its core opens a control port (config `control: {port: 0, token_file}`, 4.1); the shell's test harness starts the app with that port, and the runner drives it with the control-port driver (JSON lines, `auth` first: `crates/ffi/tests/conformance.rs`'s `Port`), from Rust (a small test binary pointed at the port) or from any language that reads `cases.json` and implements the step language above. Requests then go through the shell's own core and its real geometry, since the shell reports what it measured (`reports() == false`); what the suite can't see (the shell's drawing, its gesture recognisers) is checked by the platform's own UI tests (plan 11).

**Still to come from Draft 1's list** (16.1 then): run and `ui.waitRun`, widgets set by gesture on their rows, group and frame editing beyond moves, nets and slices, watch and viewers, Save As and reopen through asks, every ask kind, engine switching, themes and text size, `lifecycle.*`. They are cases to add as each is built in the core; the hostile sweep grows with each new parser.

### 16.2 Shared fixtures

In `crates/editor-core/fixtures/`, decoded by Rust, Swift and Kotlin tests as `crates/proto/fixtures/` are today: `scene/full.json` (every row source, widget kind, footer kind and run state, wires plain, routed, sliced and through a net, a frame, a collapsed group, an open group, two faces measured, a fit asked for) and `scene/delta.json` (one node's run state changed), both built (N2 slice 2; `scene::tests::the_shared_fixtures_are_the_scene_and_decode` checks them and `PLUMCRAFT_WRITE_FIXTURES=1` rewrites them; since slice 4 a colour with a token carries the token alone); `conformance/cases.json` (16.1). Still to come: `menu.json`, `theme-*.json`, `asks.json`, `hostile/`. Schema: `crates/editor-core/schema/shell1.json` (the scene, its delta, `geometry.set`, `ui.gesture`, hits, wire targets, `geometry.locate`, `theme.get`).

## 17. Open questions

**Answered (the maintainer, 2026-11-19),** as pasted: "Shell interface Q: 1A; 2A; 3A; 4A; 5A; 6A; 7A; 8A; 9A; 10A; 11B; 12A; 13A; 14A; 15A; 16A; 17A; 18A; 19A; 20A \"Remember this a potentially useful option for later.\"; 21A". A is each question's recommendation below, so every one stands as recommended **except 11: documents share one in-process engine from the start** (B), not one engine per document. The note on 20 is about serving the control channel on the local network: kept as an option for later. 21 is not one of these (the 2 GB floor on mobile: `docs/research/24-native-shells.md` section 15.5). The sections above still say "proposed" where these questions touched them; they are brought into line as each stage builds them.

Each with the answer recommended.

1. **`ui.invoke` and blocking dialogs.** Today `ui.invoke graph.open` replies only when the native dialog closes (`docs/control-protocol.md`). With asks it can reply at once with the ask in `asks`, or wait until the ask is answered. *Recommended:* reply at once, with the ask; a script answers it with `ask.answer`. Under egui today a script could not answer the dialog anyway, so nothing a script relies on is lost.
2. **New canvas commands and `command.list`.** N1's gate is byte-for-byte replies; adding `selection.*`, `canvas.*`, `net.select`, `viewer.size` changes `command.list`. *Recommended:* register them in N2, behind `commands.canvas`, and update the probes' expectations then.
3. **Hooks have no document.** `Hooks` are `fn()` (`commands.rs:42-59`); with two documents an application's overlays can't tell them apart. *Recommended:* N1 adds a `DocId` argument; PlumImpress's hooks are changed with it.
4. **Transient shell state in `ui.inspect`.** `canvas.editing`, `popups.anyPopup`, the rename window and the palette are UI state today. *Recommended:* the palette becomes core state (its index and search are logic, plan 4.1); rename is a `text` ask; `editing` and `anyPopup` come from `ui.report`.
5. **Viewer-local state.** Grid format, table sort, text wrap and find, PDF page and array log scale are egui-local and not commands (13.2). *Recommended:* make each a command so scripts and every shell share them; keep only per-frame state (mesh camera during a drag, pan) shell-local.
6. **Audio on mobile.** *Recommended:* the core keeps cpal (it supports CoreAudio on iOS and AAudio on Android); the shell owns the audio session category, interruptions and focus, and tells the core to stop on an interruption.
7. **Wire shape.** egui-snarl's wires are degree-5 Béziers; native toolkits draw cubics. *Recommended:* the core sends a cubic chain fitted to today's curve (two cubics per wire), and hit-tests that chain, so drawn and hit agree everywhere; egui draws the same chain from N2. *As built in N2 slice 3:* four cubics (8.8), since two strayed up to 2.2 graph units; egui-snarl still draws its own plain wires, up to 0.72 units off the chain (slice 4 put egui's faces where egui draws them, 8.11 `offset`, so its pins and the chains' ends now agree).
8. **Drop a wire on empty canvas.** It is not an egui behaviour today (egui-snarl drops it). *Recommended:* add it for every shell (plan 5.5 lists it for touch): the palette filtered to kinds that take the wire, one undo step for add and connect (a new command, `node.addWired`).
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
| `engine_view.rs` | Engine menu choices, Connector, Reloader, Installer, Attach and Install forms, `pump_engine` | 12.1, 11.4, 7.2 `engine` |
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
