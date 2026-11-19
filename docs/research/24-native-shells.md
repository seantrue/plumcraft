# 24. Native editors: one editor core, a shell per platform

**Proposal, 2026-11-19. Its twelve decisions were answered the same day, all as recommended (section 14). Stage N0 ran the same day (section 15): the boundary and the contract's first draft are there, and the canvas is measured on the Mac but not yet at 120 Hz on an iPad.**

The maintainer, 2026-11-19:

- "Plan a separate implementation of the editor portion of plumcraft on multiple ui framework/languages (MacOS/Swift, iOS/Swift, Android/Kotlin). The existing egui experience is inferior in MacOS, and terrible in iOS."
- "Attend in particular to native platform gestures and conventions."
- "The challenges seems likely to be in the node editor itself, in common definitions of platform specific code (open file, save file, etc)"
- "Assume that the existing attempt to use egui as a universal solution for hand held/tablet devices is fatally flawed, and that using it on desktops is temporarily adequate."
- "Focus on clean interfaces and performant interactions between engines, supporting code, and the gui."
- "Keep as much as reasonable of the non-gui code, and propose separating it into other crates when warranted."
- "Preserve the current capability for logging, debug driving, et al."

## 1. Summary

- **Three layers, two contracts.** Engines speak `docs/interface.md` to the session, as today. A new toolkit-free crate, the **editor core**, owns everything about editing that isn't drawing or input recognition. Each platform's **shell** (egui, Swift on macOS and iOS, Kotlin on Android) speaks a second written contract to the core, `docs/shell-interface.md`.
- **The shell interface is the control protocol, grown up.** A native shell is an in-process client of the same methods a script uses over the control channel (`command.run`, `command.list`, …), plus change notices, a canvas scene and byte buffers. So debug driving works the same on every shell, by construction.
- **The platform recognises, the core decides.** Gesture recognition, drawing, text input, menus, pickers and windows are native. What a gesture means (which wire may land on which port, what a drop into a frame does, what Tidy does) is decided once, in Rust.
- **Native drawing from a shared scene.** The core publishes the canvas as a scene (nodes, rows, wires, frames, run state). Each shell draws it with its own 2D stack, so text, controls, accessibility and compositing are the platform's. Pan, zoom and drag never cross the boundary per frame.
- **egui becomes the first shell of the core**, not a casualty. It stays the Linux and Windows editor and carries the existing 1,180-odd tests while the core is cut out from under it. That is how the seam is proven before any Swift is written.
- **Order proposed:** cut the core (N1, N2), then the Apple shells from one Swift package with iPad setting the scope and a Mac target alive from the first day (N3, N4), then Android (N5), then PlumImpress on them (N6).

## 2. What is there today

Facts from a survey of this repository, the iPad spike and the native virtimp.com app, 2026-11-19.

### 2.1 The seam is mostly at `plumcraft-session` already

- **In the session, no egui:** the graph, the 111 or so commands (`CommandSpec { id, label, menu, shortcut, params, enabled, run, journal }`, `run: fn(&mut Session, &Value) -> Result<Value>`), undo and redo, the run state and its timed log, the viewer and pop-out *state*, hooks, forms, named ports, the journal.
- **In `plumcraft-ui` (23.7k lines), tied to egui:**
  - the selection (`canvas.picked`) and the viewport (`canvas.to_global`, an egui transform);
  - node sizes and pin positions, measured by egui-snarl, and all hit testing (`edit_view.rs:134`, `run_view.rs:280`, `wire_route.rs:98`);
  - `EditorApp::invoke` (`lib.rs:961`): what the UI fills in for a command (dialogs, the discard prompt, the selection, measured sizes for Tidy), and `try_run`'s handling of results;
  - the engine event pump, `Link::start(conn, label, Option<egui::Context>)`, whose only use of egui is the repaint wake;
  - `pump_runs`, which calls `conn.call("run")` **on the UI thread**;
  - decoding to textures (`media.rs`, `audio.rs`, `compare_view.rs`), the 17 viewers, themes, fonts, preferences, the clipboard, the `ui.*` control methods.
- **Pure logic stranded in the UI crate:** `prefs.rs`, `run_anim.rs`, `playback.rs`, `viewers/yaml.rs`, and nearly `appearance.rs`, `diff.rs`, `probe.rs`, the geometry in `wire_route.rs`, the palette's index, the text formatters in `run_view.rs`.
- **Windowless use pulls in egui:** `apps/plumcraft-editor/src/headless.rs` imports `plumcraft_ui::link::Link`.
- **Platform services:** file dialogs are behind a trait (`FileDialogs`, `lib.rs:130`, rfd behind it). The clipboard is direct egui calls. There is no file drag and drop, no recent files, no autosave, no printing, and one `Session` per process. Opening a file in another app shells out to `/usr/bin/open` or `xdg-open` from the session.
- **Logging:** there is no logging framework. Diagnostics are `eprintln!`, the status bar's messages, an engine's `stderr_tail`, and the session's journal.
- **Debug driving:** the control channel (`docs/control-protocol.md`): loopback TCP, token, JSON lines. `plumcraft_automation::dispatch` serves the session-only methods without a window; `crates/ui/src/control.rs` serves `ui.*` on the UI thread. `--screenshot` and an offscreen render through egui_kittest. No `ui.pointer` or `ui.key`.

### 2.2 The iPad spike (PlumImpress `spike/ipad-editor`, PlumCraft `spike/ios`)

It is the egui editor on iOS through winit and wgpu, with the engine in-process, and a small Swift file for pickers and sharing behind a C bridge. It ran on the simulator, an iPhone 15 Pro and an M4 iPad Pro. What it cost and what it left open:

- **Cost:** a patched winit (0.30.13 predates UIScene: a black screen, then a zero-width window); wgpu limits taken from the adapter; a logger, because a fatal error was only logged before `exit(0)`; file dialogs reworked to answer later (`waiting()`, `take_answer()`); `COMMONKIT_STORE_ROOT` pointed at the app's caches, because the default is `/dev/shm`.
- **Limits that are the platform's, not egui's:** no subprocesses, so only in-process engines and no craft engines; memory (a full-size photo was killed; inputs are capped at 1280 px there).
- **Left open, and these are egui's:** the menu bar under the status bar (safe areas), long press, pinch, two-finger pan, hit-target sizes, the on-screen keyboard. Every test went through the control channel; touch was never exercised.
- **Worth keeping:** the deferred-answer dialogs, the `SendTo` trait, the Swift picker code, the rule that an iOS build hosts engines in-process, the store-root fix, the control channel working from the Mac.

### 2.3 The native virtimp.com app

SwiftUI (one multiplatform target, iOS 18 and macOS 15, Swift 6), Kotlin with Compose (minSdk 29), and an egui desktop, all over one Rust static library.

- **What it proves:** the build and packaging path. A Rust `staticlib` in an xcframework behind a Swift package, built for macOS, iOS and the simulator; the same crate as a `cdylib` under `jniLibs` with hand-written JNI shims; a hand-written C ABI of five functions carrying JSON (`char *vl_render(const char *request_json)`, `vl_free`), with `catch_unwind` at the edge; pickers, Photos, share sheets and `FileProvider` on each platform; shared fixtures across the three.
- **What it does not prove:** it links the research software's renderer directly, with no PlumCraft crate, no session and no protocol 2. Images cross as file paths. There are **no callbacks, no progress and no cancellation**: a render blocks, and progress is inferred by polling marker files. An editor needs all three.
- **Lessons recorded there:** JSON goes over JNI as byte arrays (JNI strings are modified UTF-8); a Swift 6 main-actor closure trapped on a framework's queue; each render needs a 16 MB stack; the Android emulator has no GPU.

## 3. Architecture

```
 engines ── docs/interface.md ── host / coordinator / store
                                        │
                                 plumcraft-session        the document: graph, commands, undo, run state
                                        │
                                 plumcraft-editor-core    NEW: everything about editing that isn't drawing or input recognition
                                        │
                        docs/shell-interface.md           NEW: the second contract
              ┌──────────────┬──────────┴──────────┬──────────────┐
          egui shell     Apple shell (Swift)   Android shell    control channel
        (plumcraft-ui)   macOS · iPadOS · iOS    (Kotlin)       (scripts, agents, tests)
              in-process Rust     through plumcraft-ffi (C ABI)      loopback TCP
```

Rules, in the manner of the golden rules:

1. **No toolkit in the core.** `plumcraft-editor-core` names no egui, AppKit, UIKit or Android type. Colours, points and rects are its own plain types.
2. **No editing logic in a shell.** A shell holds nothing a second shell would have to copy: no type checks, no selection rules, no command enabling. If two shells would both need it, it belongs in the core.
3. **Everything is still a command.** A shell's menu, shortcut, context menu, toolbar and gesture all end in a command id, as rule 4 of `AGENTS.md` says. A gesture that changes the document is a command with parameters.
4. **The shell interface is a contract.** A change to it changes `docs/shell-interface.md` in the same commit; additions are capabilities.
5. **A shell is hostile input too.** The core never panics on what a shell sends; the FFI edge catches unwinds; the shell's parsers of core data skip what they don't know.
6. **The UI thread never waits for an engine.** No shell call blocks on engine I/O.

## 4. The editor core

### 4.1 What moves into it

| From | What | Note |
|---|---|---|
| `ui/lib.rs` `invoke`, `try_run` | Filling a command's parameters from UI state; prompts; result handling | Becomes a state machine that *asks* the shell (section 6) instead of calling a dialog |
| `ui/link.rs` | The event pump | `egui::Context` becomes a `Waker` (a callback with no arguments) |
| `ui/lib.rs` `pump_runs`, `drain_engine` | Sending runs, applying events | Moves off the UI thread (4.2) |
| `ui/engine_view.rs` `Connector` | Switching, adding, installing engines | Same |
| `ui/canvas.rs` `picked`, `focus` | Selection, group focus | The core owns selection; `ui.inspect` reads it there |
| `ui/edit_view.rs`, `run_view.rs`, `wire_route.rs` | Hit testing, wire paths, align and distribute, copy and paste | On geometry the shell reports (5.2) |
| `ui/control.rs` | `ui.inspect`, `ui.invoke`, `ui.prompt`, `ui.set`, `ui.waitRun` | The core answers; only a screenshot needs the shell |
| `ui/prefs.rs`, `appearance.rs`, `theme.rs` (tokens) | Preferences, themes as data | `Color32` becomes a plain RGBA |
| `ui/run_anim.rs`, `status_msgs.rs` (order), `app_hooks.rs`, `palette.rs` (index), `watched.rs` (`Fulls`), `probe.rs`, `diff.rs`, `viewers/yaml.rs` | As named | Mostly as they are |
| `ui/lib.rs` `MENUS`, `checked`, `parse_shortcut` | The menu model | Shortcuts as `{modifiers, key}`, mapped per platform |

What stays in a shell: drawing, measuring text, recognising gestures, native controls, windows, pickers, textures.

### 4.2 Threads

- **The core runs on its own thread** and owns the `Session`. Engine calls go to workers, as saves already do.
- **Commands are asynchronous for a native shell:** `request` returns at once and the reply arrives with the next wake. In-process Rust (the egui shell, tests) may also call synchronously, as today.
- **Reads on the UI thread are lock-free:** the scene and the menu model are immutable snapshots behind an atomically swapped pointer. A frame reads the current one and never waits.
- **One wake, no payload.** The core calls the shell's `wake()` (from any thread; the shell hops to its main thread and coalesces). The shell then asks what changed. This is `Link`'s inbox design, kept: no data crosses on a foreign thread, which also avoids the Swift 6 isolation trap recorded in 2.3.

### 4.3 More than one document

macOS expects several windows, iPadOS several scenes, and today there is one `Session` per process.

- **Proposed:** a process-wide `Core` (preferences, the engine list, the store, the command registry) and one `Document` per window, each with its own `Session` and engine link.
- **To check in N1, not known today:** whether anything besides the command registry and hooks is process-global in a way that two sessions would trip over, and whether two sessions may share one in-process engine.

### 4.4 Other crates

| Crate | Status | Holds |
|---|---|---|
| `plumcraft-editor-core` (`crates/editor-core`) | New | Section 4.1 |
| `plumcraft-media` (`crates/media`) | New | Decoding to plain rasters on workers: images, SVG (over `plumcraft-svg`), PDF pages (hayro), mesh shading, spectrogram and waveform peaks, compare pictures. A feature per decoder, so a platform that uses its own (PDFKit) leaves one out |
| `plumcraft-ffi` (`crates/ffi`) | New | The C ABI of section 7. The only `unsafe` |
| `plumcraft-ui` | Shrinks | The egui shell: canvas, widgets, viewers' drawing |
| `plumcraft-automation` | Small change | The server as it is; `ui.*` goes to the core |
| `apps/plumcraft-editor` | Small change | `headless.rs` uses the core, so windowless use stops building egui |

Audio output stays behind a small trait (`AudioOut`): cpal on desktops, the platform's own on iOS and Android, where the audio session and focus rules are the platform's.

## 5. The node canvas

This is the hard part, and where egui fails worst on touch.

### 5.1 Three ways to draw it

| | A. Native drawing from a shared scene | B. One Rust GPU renderer in a native view | C. Each platform on its own |
|---|---|---|---|
| Text, text input, IME | Native | Ours to build, again | Native |
| Accessibility | Native elements | Ours to build | Native |
| Looks the same everywhere | Geometry and colour yes, text no | Yes | No |
| Logic written | Once | Once | Three times |
| Drawing code written | Twice (Swift, Kotlin) | Once | Twice |
| Risk | The scene's shape | Rebuilding egui's problems | Three editors drifting apart |

**Recommended: A.** B keeps the faults that sank egui on touch (text input, keyboard, accessibility, system gestures), and C triples the part that is hardest to get right.

### 5.2 The scene

The core publishes a scene in graph coordinates. Each item carries a revision, so a shell redraws only what changed.

- **Node face:** id, position, title, family colour, engine mark, a list of **rows** (an input or output with its type colour and caption; a widget with its kind, value, range and choices; a preview with an image reference), the run state as shown (`waiting`, `running` with a fraction, `done`, `cached`, `failed`, `skipped`, `stale`, `not run`), overlays and footers from hooks.
- **Wire:** its ends, its type colour, its path as bezier points, marks from hooks, a net label.
- **Frames, groups and proxies, the selection, the marquee, a wire being dragged** and where it may land.

**Who measures:** the shell. A node's size depends on native text and controls, so the shell lays out each face from its rows and reports `size` and pin anchors back (`geometry.set`), as the canvas reports measured sizes to Tidy today. The core does routing, hit testing, snapping, Tidy and align on those numbers. So what is drawn and what is hit always agree, on every platform.

### 5.3 What must never cross per frame

- **Pan and zoom** are the shell's: a transform on a layer, at the display's rate. The core hears the viewport when a gesture ends (and at most a few times a second during it) for `ui.inspect` and `ui.set`.
- **Dragging nodes:** the shell moves the layers itself. Wires on a moving node need new paths every frame: one typed call, `wire_paths(nodes, offset) → floats`, no JSON, a few microseconds. The drop is one command (`node.move`), so one undo step.
- **Dragging a wire:** the shell tracks the finger; the core answers `wire_target(point)` (the port to snap to and whether its type fits) from the last geometry.
- **Run progress** is coalesced in the core to at most one scene change per display frame.

### 5.4 Drawing it natively

- **Apple (one implementation for macOS and iOS):** a layer per node with its face drawn once into it, wires in shape layers, all under one transformed layer. Pan, zoom and drag are then the compositor's work. Faces are redrawn at the new scale when a zoom settles, for sharp text. Core Animation and Core Graphics are the same on both systems; only input differs.
  - **As N0 measured it (15.2):** one shape layer per wire, its frame tight around the wire, never batched into graph-sized layers. Only the visible faces are redrawn after a zoom settles, off the main thread, at a scale that steps in powers of two. Run state goes on sublayers (a wash and a progress bar), never into a redrawn face.
- **Inline controls use the field-editor pattern,** as AppKit itself does: a widget at rest is drawn into the face; on focus one real native control (a text field, a slider, a picker) is placed over it. That gives native text input, the keyboard and its avoidance, without thousands of live controls.
- **Android:** Compose, a custom `Canvas` with a graphics layer per node, the same field-editor pattern with one `BasicTextField`.
- **Accessibility:** a node is an element (its title and state), its ports are children, its commands are custom actions. Wiring by taps (5.5) makes the canvas usable with VoiceOver and TalkBack. egui cannot do this today (`control-protocol.md`, known gaps).

### 5.5 Gestures and conventions

One meaning per row, native recognition per column. "Command" names what the core receives.

| Meaning | macOS | iPadOS and iOS | Android |
|---|---|---|---|
| Pan | Two-finger scroll; Space-drag; middle drag | One finger on empty canvas; two fingers anywhere; trackpad scroll | One finger on empty canvas; two fingers anywhere; mouse wheel |
| Zoom | Pinch; ⌘-scroll; ⌘+ ⌘− ⌘0; smart zoom (two-finger double tap) fits a node | Pinch; double tap fits a node; ⌘+ ⌘− with a keyboard | Pinch; double tap; Ctrl-scroll |
| Select | Click; Shift- and ⌘-click; drag on empty canvas is a marquee | Tap; a Select mode in the toolbar, then tap or drag a marquee; Pencil drags a marquee directly; two-finger tap adds | Tap; long press enters selection mode with a contextual bar |
| Move | Drag; arrows nudge | Drag (after a short hold when panning would be ambiguous) | Drag after long press, or drag in selection mode |
| Wire | Drag from a pin | Drag from a pin with a 44-point target, magnetic snap to ports that fit, a haptic on snap; **or tap a port, then tap another** | The same, 48 dp targets |
| Context menu | Right click, Control-click | Long press (a context menu with a preview); secondary click with a pointer | Long press; right click with a mouse |
| A port's value (hover today) | Hover | Pointer or Pencil hover; a tap shows a popover with Watch | Stylus or mouse hover; a tap shows a sheet |
| Add a node | Double click on empty canvas; the menu; drag from the palette | The + button; drag from the palette; drop a wire on empty canvas | The floating action button; drop a wire on empty canvas |
| Undo, redo | ⌘Z, ⇧⌘Z, the Edit menu with the step's name | Three-finger swipe and tap; ⌘Z; shake is off | Toolbar buttons; Ctrl-Z |
| Open a value | Double click a preview: a window | Double tap: a full-screen cover or a new window (Stage Manager) | Double tap: a full-screen pane |
| Leave | Close the window | Swipe down a sheet; close the scene | **Back:** closes a sheet, then leaves group focus, then the document (predictive back) |

Conventions beyond gestures:

- **macOS:** the real menu bar built from the command list, validated by `enabled` with the reason as the item's tooltip; the document in the title bar with its proxy icon and edited dot; window tabs and full screen; a Settings window on ⌘,; an inspector sidebar and a toolbar; files dragged in from Finder and values dragged out; Quick Look on Space for a watched value; the system's undo manager carrying the core's step names; Services and Share.
- **iPadOS:** the document browser and open-in-place from Files; several scenes and Stage Manager; the menu bar and `UIKeyCommand`s from the same command list (so the shortcut overlay is right); pointer effects with a trackpad; Pencil with hover, and PencilKit-class input for PlumImpress's mask painter; drag and drop between apps (a photo in becomes a load node through the application's opener; a preview out is a PNG); the share sheet; safe areas and the keyboard layout guide; Dynamic Type; sheets with detents for the palette, inspector and Watched panel in compact width; state restoration.
- **iPhone:** the same shell in compact width: the canvas full screen, everything else in sheets, a command palette in place of menus.
- **Android:** Material 3 with window size classes (panes side by side on tablets and foldables, sheets on phones); edge to edge with IME insets; the Storage Access Framework and the photo picker; receiving `ACTION_VIEW` and `ACTION_SEND`; the keyboard shortcuts helper; mouse and stylus on tablets, ChromeOS and DeX; restoring after the process is killed.
- **Themes:** Paper, Pewter and Dusk stay the palettes; a new default, "follow the system", picks Paper or Dusk. Text sizes follow the system's setting, scaled by `view.textSize`.

## 6. Platform services: one definition, a native answer each

The core never calls the platform. It **asks**, and the shell answers later. This generalises three things that exist: `Views::ui_asks`, the spike's deferred `FileDialogs`, and `SendTo`. An ask is `{id, kind, params}` in the change feed; the answer is a call back with that id. A script answers them over the control channel, as `ui.prompt` does now.

| Ask | macOS | iOS | Android | Core sees |
|---|---|---|---|---|
| `file.open {kinds, multiple}` | `NSOpenPanel` | Document picker, open in place; photo picker for pictures | SAF `OpenDocument`; photo picker | Paths it can read (below) |
| `file.save {name, kind}` | `NSSavePanel` | Export through the document picker | SAF `CreateDocument` | A path to write; `file.commit` when done |
| `folder.pick` | `NSOpenPanel` | Document picker (folders) | SAF `OpenDocumentTree` | A workspace root |
| `prompt {kind, text, answers}` | Sheet on the window | Alert or action sheet | Dialog | The answer |
| `form {CommandForm}` | Sheet | Sheet with detents | Bottom sheet or dialog | The fields |
| `clipboard.get` / `set` | `NSPasteboard` | `UIPasteboard` | `ClipboardManager` | Text, or a typed payload |
| `share {file, subject, target?}` | `NSSharingService` | Share sheet, mail | `ACTION_SEND` with `FileProvider` | A notice |
| `open.external {file or url, app?}` | `NSWorkspace` | `UIApplication.open`, Quick Look | `ACTION_VIEW` | Done or the reason |
| `audio.out` | cpal or AVAudioEngine | AVAudioEngine | Oboe or AudioTrack | An `AudioOut` |
| `screenshot` | The window's view | The scene's view | `PixelCopy` | A PNG path |

**Files are the hard case.** Engines read paths, and two platforms don't hand out paths.

- **The rule:** the core and engines only ever see ordinary paths inside roots the shell declares at start (`Dirs { documents, caches, store, scratch, prefs, logs }`).
- **macOS, not sandboxed (today's Developer ID build):** the picked path, as now.
- **iOS:** open in place with a security-scoped bookmark kept by the shell for the document's life, or, for a photo, a copy into the app's imports (what the spike does).
- **Android:** a content URI has no path, so the shell copies it into the app's files, named by its digest so a second pick costs nothing; a save writes to a path in scratch, then the shell streams it to the URI on `file.commit`.
- **A grant is the shell's to keep.** The core stores a path and an opaque `grant` string beside it in recent files; the shell turns the grant back into access after a restart.

**Capabilities.** At start the shell declares what the platform has: `{subprocess, craft_apps, python, multi_window, pointer, keyboard, hover, pencil, max_picture_px}`. Commands that need a missing one are disabled with the reason (the mechanism is there: `enabled` returns a `String`). So on iOS "Text · Python · process" is listed and says why not, instead of failing.

**Engines on a phone or tablet.** In-process Rust engines, and any HTTP engine (`engine.attach {url, token}` exists): an iPad editing a graph that runs on a Mac or a server is already in the protocol. No process, Python or craft engines.

**Life cycle and memory.** The shell tells the core `background`, `foreground` and `memory {level}`. The core drops caches (`Fulls`, decoded rasters) on a warning and saves the journal on `background`. A run in flight when iOS suspends is asked for background time and otherwise cancelled cleanly. `max_picture_px` goes to engines as a hint, replacing the spike's private cap.

**Documents on mobile.** Desktop keeps explicit Save. On iOS and Android the convention is no Save command: propose autosave to the document's file after each undo step settles, with the journal as crash recovery.

## 7. The boundary (`plumcraft-ffi`)

A hand-written C ABI, as the native virtimp.com app has, with the three things it lacked.

```c
plum_core *plum_open(const uint8_t *config_json, size_t len, void (*wake)(void *), void *ctx);
void       plum_close(plum_core *);

// Cold path: the control protocol's methods, JSON in and out. Asynchronous; the reply comes in plum_poll.
uint64_t   plum_request(plum_core *, const uint8_t *json, size_t len);
plum_buf   plum_poll(plum_core *);              // replies, changed domains, scene changes, asks, log lines

// Hot path: typed, synchronous, lock-free reads of the current snapshot.
size_t     plum_wire_paths(plum_core *, const uint64_t *nodes, size_t n, float dx, float dy, float *out, size_t cap);
plum_hit   plum_hit_test(plum_core *, float x, float y, float slop);
plum_hit   plum_wire_target(plum_core *, uint64_t from_pin, float x, float y);

// Bytes: borrowed, not copied. Valid until released.
plum_bytes plum_blob(plum_core *, const char *handle);   // {ptr, len, w, h, format, token}
void       plum_release(plum_core *, uint64_t token);
void       plum_free(plum_buf);
```

- **Why not generate it (UniFFI):** the cold surface is one function, because it is already JSON commands; the hot surface is three; and bytes must not be copied, which a generator's buffers do. Hand-written, it is about a dozen functions, one mechanism for Swift and JNI, and the precedent builds on all three platforms today. UniFFI is the alternative if the typed surface grows (decision 2).
- **JSON on the cold path is a choice to measure, not assume.** It makes every shell call visible to the control channel and to a log, and typed on each side by the schema (`schema/shell1.json`, generated as `protocol2.json` is; Swift `Codable` and Kotlin serialization types generated from it). N0 measures a 500-node scene; if it misses the budget in section 10, the scene alone moves to a compact binary form and the rest stays.
- **Bytes.** A blob from the store is often a mapped segment already (`Bytes::Mapped(Arc<Segment>)`). `plum_blob` hands out the pointer and holds the `Arc` until `plum_release`: on Apple it backs a `CGDataProvider` with no copy; on Android it is a direct `ByteBuffer`, and the one copy is into the `Bitmap`. Handles are content addresses, so they are the shells' texture-cache keys.
- **Full-size data never crosses for display.** The watch's `px` mechanism already asks an engine for a picture the size of the view; the shell sets it from the view's pixel size.
- **`unsafe`.** A C ABI cannot be written without it. Golden rule 2 would gain one more named exception: `crates/ffi`, with a `SAFETY:` comment on every block, `catch_unwind` on every entry, and nothing else in that crate (decision 3).
- **Applications of the editor.** PlumImpress's Rust side (its engine pack, commands, forms, openers, hooks) is toolkit-free already and is passed to the core as today's `AppConfig` minus its drawing parts. It builds its own static library with a one-line export of the ABI. Its custom value editors (curves, the mask painter) and any custom viewer are drawing code: they are registered in the Swift and Kotlin kits by kind, with their model in Rust. They must be written again natively; that is the main cost to PlumImpress.

## 8. Viewers

Seventeen viewers and a compare view are egui code today. Each splits the same way as the canvas:

- **In the core and `plumcraft-media`:** choosing the viewer, its state, its tools as commands (`viewer.probe`, `viewer.zoom`, `viewer.compare`, `viewer.sheet` exist), decoding and derived pictures on workers, exact probe values from the array (never from the 8-bit picture).
- **In the shell:** putting pixels, text and grids on screen, natively.

Order, by what the floor needs first: `picture`, `text.plain`, `scalar`, `items`, `array.pictures`, `table.grid`, `array.grid`, `audio`, `svg`, `tree` and `yaml`, `hex`, then `array.plot`, `pdf.pages`, `doc`, `xml`, `mesh`. A shell that lacks one falls back to the engine's rendition as a picture, which every value already has. A platform may use its own decoder where it is better (PDFKit for `pdf.pages`), since the viewer's state is the same (decision 9).

## 9. Logging and debug driving

Nothing that works today is lost; two gaps are closed because phones need them.

- **The control channel moves into the core** and so exists in every shell. Its methods and replies don't change. `ui.inspect` is built from the core's state; the shell adds only what it alone knows (window rects, the texture state of a viewer).
- **`ui.screenshot`** becomes an ask the shell answers. The egui offscreen fallback stays in the egui shell; a native shell renders its own view hierarchy offscreen.
- **New, closing a known gap:** `ui.gesture {kind: tap | doubleTap | longPress | drag | pinch | pan, at, to?, scale?}` enters where a recognised gesture does, so a script exercises the core's decisions on every shell. Recognition itself is tested by the platforms' own UI test tools.
- **Reaching it:** the simulator shares the Mac's loopback (the spike used this). Android: `adb forward`. An iOS device: a debug build may bind to the local network, with the token (decision 10); a release build never does.
- **Logging:** the core logs through one facade with levels and a target. Sinks: stderr on desktops (today's behaviour), the unified log on Apple, logcat on Android, a bounded file under `Dirs.logs`, and a ring the control channel reads (`log.tail {since, level}`). An "Export Diagnostics" command shares the log, the journal and the engine's `stderr_tail`. On iOS, stderr goes nowhere a user can see, which is why the spike needed its own logger.
- **The journal, `--screenshot`, `plumcraft run`, the probes in `claude-debug/`** work as now; the probes are the first conformance cases (section 11).

## 10. Budgets

Numbers to hold each stage to. All are proposals until N0 measures them. **How the canvas rows are checked (N0, 15.2):** by Instruments' Animation Hitches, as hitch time per phase, not by the display link's intervals, which missed late frames from the render server. The canvas row is read at 500 active nodes and 800 wires (the maintainer: "Initial target would be < 500 active nodes").

| What | Budget | How it is met |
|---|---|---|
| Pan, zoom, node drag | Every display frame (8.3 ms at 120 Hz), on a 500-node, 800-wire graph | No boundary crossing but `plum_wire_paths`; compositor transforms |
| A command (an edit) to its scene change on screen | 16 ms | Asynchronous request; scene changes by revision |
| Run progress | One scene change per frame at most | Coalesced in the core |
| A preview on screen after its event | One frame after decoding | No copy from the store on Apple; one on Android |
| Launch to a usable window | Under 1 s; engine connecting shown as a state | Nothing on the main thread waits for an engine |
| Main-thread stall, ever | None over 100 ms | Rule 6 |
| Memory on a warning | Decoded caches dropped at once | `memory {level}` |

## 11. Tests

- **Rust, on Linux CI as now:** the core's logic, which is most of it, with the existing unit tests and kittests moving with the code. A new hostile sweep for everything a shell can send.
- **A shell conformance suite:** scripted scenarios over the control channel (add, wire, run, watch, undo, group, save, reopen, the asks answered by script, `ui.gesture`), checked against `ui.inspect` and `graph.get`. Every shell must pass the same list; egui passes it first. It grows from `claude-debug/editor/control_probe.py` and its siblings.
- **Shared vectors:** scene and menu-model fixtures in `crates/editor-core/fixtures/`, decoded by Rust, Swift and Kotlin tests, as `crates/proto/fixtures/` are shared today.
- **Swift:** package tests and UI tests run **locally on the Mac only**, reported as evidence, since there is no macOS CI. Screenshots through the control channel after Tidy and fit.
- **Kotlin:** JVM tests and a Linux build of the library in CI; instrumented tests on a local emulator or device.

## 12. Stages

Milestone N. Each stage ends at something that runs and a gate.

| Stage | What | Gate |
|---|---|---|
| **N0. Two spikes and the contract's first draft** | (a) A Swift canvas on fake scene data: 500 nodes, 800 wires, on a Mac and an iPad. (b) The core thread, `wake`, one JSON command and one blob through an xcframework and through JNI, with the text engine in-process, reusing the native app's build scripts. (c) `docs/shell-interface.md`, draft | The budgets of section 10 measured; decisions 1 and 2 confirmed or reversed by numbers |
| **N1. Cut the core** | `plumcraft-editor-core` and `plumcraft-media`; the egui shell on top; engine calls off the UI thread; `Waker`; the asks; the logging facade; `headless.rs` off egui; the two-sessions audit | Every existing test and probe passes unchanged; the control protocol's replies are byte-for-byte what they were; no user-visible change |
| **N2. Selection, scene and geometry in the core** | Selection and hit testing move; the scene; `geometry.set`; wire paths; `ui.gesture`; `plumcraft-ffi`; the schema; the conformance suite | egui draws from the scene and passes the suite; the FFI passes it headless |
| **N3. The Apple kit, read and run** | One Swift package: the FFI wrapper, observable models, the layer canvas, the menu builder, the asks. A Mac app and an iPad app that open, show, pan, zoom, run and watch, with `picture`, `text.plain` and `scalar` | The read-only half of the suite on both; budgets met on device |
| **N4. Apple editing** | Every gesture of 5.5, inline controls, palette, inspector, groups, frames, nets, named ports, settings and themes, pop-outs and scenes, files and documents, remaining viewers by the order in 8. iPad sets the scope; the Mac app follows it, then iPhone's compact layout | The whole suite; the floor's list in `docs/plan.md` section 9 walked on each; a notarized Mac build and a TestFlight build |
| **N5. Android** | The Kotlin kit and app on the same core: tablet, then phone | The whole suite on an emulator and a device |
| **N6. PlumImpress on the shells** | Its curves and mask editors in Swift and Kotlin; its openers and Send To through the asks. The spike branches and the winit patch are retired | Its own floor walk |

N1 is the largest piece of Rust and changes nothing a user sees, so it goes in slices behind the existing tests: the waker and pump; runs off the UI thread; preferences and themes as data; the asks under `invoke`; the control methods; the media crate; the menu model. egui stays the editor on Linux and Windows throughout, and on the Mac until N4's gate.

## 13. Risks

- **The scene's shape is wrong for one toolkit.** N0's canvas spike and writing the egui shell against the scene first (N2) are there to find out early.
- **Inline widgets.** The widest surface in the editor (every parameter kind, twice more). The field-editor pattern bounds it; the order follows the catalogue's kinds by use.
- **Two sessions in one process** may trip on process-wide state (4.3). Not known until audited.
- **JSON on the cold path** may be too slow for a large scene. Measured in N0, with a named fallback.
- **Three shells drift.** Rule 2 and the conformance suite are the defence; a feature isn't done until the suite covers it.
- **No macOS CI** means the Swift shells are checked only by local runs. The more that lives in the core, the less that matters.
- **Memory on phones** is an engine's problem that the editor can only bound (`max_picture_px`, caches dropped on a warning).
- **Size.** This is a second and third editor. Viewers and widgets dominate; the order in sections 8 and 12 puts a useful app (open, tweak, run, look) before the long tail.

## 14. Decisions for the maintainer

**Decided (the maintainer, 2026-11-19): every one as recommended.** The answer, as pasted from the checklist page (<https://claude.ai/artifact/UJEDs4cSfSRTerTt8G5kvb>): "Native shells D24: 1A; 2A; 3A; 4A; 5A; 6A; 7A; 8A; 9A; 10A; 11A; 12A". Option A is the Recommended column on each row below. (The "D24" in that line is this note's number, not `docs/plan.md`'s D24.)

| # | Decision | Recommended (**decided**) | Otherwise (not taken) |
|---|---|---|---|
| 1 | How the canvas is drawn | Native drawing from the core's scene (5.1 A) | One Rust GPU renderer in a native view; or each platform on its own |
| 2 | The boundary | A hand-written C ABI carrying the control protocol's JSON, with typed hot calls and borrowed bytes | UniFFI-generated bindings |
| 3 | `unsafe` | A second exception to golden rule 2, for `crates/ffi` only | The FFI crate in a repository of its own |
| 4 | Which shell first | iPad sets the scope, a Mac target alive from the first day; Mac ships just after | Mac first; or both at once |
| 5 | iPhone | The same shell in compact width, after iPad | iPad only for now; or a reduced "tweak and run" app |
| 6 | Where the native code lives | This repository, `native/apple` and `native/android` | A repository per platform |
| 7 | Saving on iOS and Android | Autosave, no Save command, the system's document browser | Explicit Save as on desktops |
| 8 | Windows | A document per window, several per process | One document per process, as now |
| 9 | Decoders | Rust decoders by default; a platform's own where it is better (PDFKit) | Rust only, for identical pictures everywhere |
| 10 | The control channel on an iOS device | Debug builds may bind to the local network, with the token | Simulator and Mac only |
| 11 | egui's future | The Linux and Windows shell and the test bed, on the core | Frozen as it is, with the core forked from it |
| 12 | Oldest systems | iOS 18, macOS 15, Android 10 (API 29), as the native virtimp.com app | Newer or older |

## 15. N0: what the spikes found (2026-11-19)

Three pieces, on branch `feat/native-n0`. Reports: `claude-debug/native-n0-ffi.md`, `claude-debug/native-n0-canvas.md`. The contract's first draft is `docs/shell-interface.md`. Machine: a MacBook Pro M4 Max, 120 Hz.

### 15.1 The boundary (decision 2: confirmed)

`crates/ffi` is a hand-written C ABI over a real session with the text engine in-process. The same flow (open, list commands, build a graph, run it, receive node status, borrow a blob) passes from Rust, from Swift on macOS and the iOS simulator, from a host JVM through JNI, and from Rust on an Android emulator. Not run: an iOS or Android device; JNI on Android itself.

| What (medians) | Rust | Swift | JVM |
|---|---|---|---|
| Request to reply, a trivial command | 6.7 µs | 24 to 52 µs | 27.5 µs |
| `graph.get`, 500 nodes and 800 wires (96 KB) | 1.47 ms | 1.74 ms | 1.53 ms |
| `graph.get`, 2,000 nodes and 3,200 wires (388 KB) | 5.95 ms | 6.6 ms | 6.1 ms |
| Borrow a 64 MiB blob stored plain | 59 µs | 75 to 96 µs | 285 µs |

- **JSON is fast enough for the scene, sent by revision.** The compact binary fallback of section 7 is not needed. Swift decodes with `JSONSerialization`; `Codable` is three to four times slower.
- **Borrowing is zero-copy for a value stored plain.** A compressible value is stored deflated, so borrowing it inflates and copies (12 ms for 64 MB). So the core resolves blobs on a worker and announces them with a wake; `plum_blob` hands out only what is already resolved.
- **Changes to section 7's sketch:** a core is a number, not a pointer (a second close is an error, not a use after free); `plum_release` takes no core, since a Swift `Data` may outlive it; strings cross as pointer and length; on Android the wake signals a condition variable and never calls into the Java VM. Still to add: a document argument (decision 8) and the hot calls, which wait for the scene (N2).
- **Two cores in one process work** (graphs, runs and events stay apart), which answers part of section 4.3. Both in-process engines report the same session id; that goes to N1's audit.

### 15.2 The canvas (decision 1: stands on the Mac; the iPad at 120 Hz is not measured)

A layer per node with its face drawn once, wires in shape layers, one transformed layer over all: the main thread does under 1.3 ms of work a frame in every phase.

- **The display link is the wrong clock.** It reported 120 Hz held in every wire mode. Instruments' Animation Hitches showed the render server late on up to a third of frames. **Hitch time per phase is the gate from here on.**
- **Wires: one shape layer each, its frame tight around the wire.** At 500 nodes and 800 wires that gives 0 to 17 ms of late frames a second across pan, zoom, drag and run progress (under 5 is good, over 10 poor). Batching wires by colour into graph-sized layers gives 280 to 495.
- **It bends at 2,000 nodes:** 103 ms a second panning and 421 with the whole graph on screen. Large graphs need culling and a simpler face when zoomed out; that is not built or measured.
- **Faces:** redraw only the visible ones, off the main thread, after a zoom settles (14 to 21 ms for 10 to 30 faces). Redrawing all 500 at screen scale takes 232 ms and 608 MB.
- **The iPad Pro (M4), the real device:** signed, installed and ran; nothing dropped at 500 or 2,000 nodes, but its display link ran at **60 Hz**, and no Instruments trace was taken on it. So the budget that matters most, every frame at 120 Hz on an iPad, is unmeasured.

- **The maintainer, 2026-11-19, to the agent running the spike (as its report records it, `claude-debug/native-n0-canvas.md` section 7a):** "Initial target would be < 500 active nodes. Current rendering is adequate." So culling and a simpler zoomed-out face wait until graphs pass about 500 active nodes, and section 10's canvas budget is read at 500 nodes and 800 wires.

### 15.3 What the contract's draft corrected in this note

- **Rule 3 is not met today.** The selection, the Edit menu's canvas items, Fit, the palette, entering a group, Esc cancelling a run and selecting a net are egui code, not commands. They get command ids in N2.
- **Wires are degree-5 Beziers** (egui-snarl), which native layers don't draw. The core sends a fitted cubic chain and hit-tests the same chain; egui draws it too from N2.
- **Section 5.2's run states mixed two things:** the node's real state (queued, started, progress, preview, done, cached, error, skipped, cancelled, with stale and not-run flags) and what the run animation shows.
- **Section 6's table of asks was short:** the Open With picker, theme import and export, the engine's save formats, several files at once, a start folder per purpose, the rename, port-name, net-name and slice editors, the Attach and Install forms, and reading the clipboard.
- **The session opens other apps itself** (`/usr/bin/open`, `xdg-open`); that becomes the `open.external` ask.
- **Section 4.3's globals were short:** forms, viewers, value editors, openers and the hash algorithm are process-wide too, and `Hooks` are `fn()` with no document.
- **Hover carries more than a port's value:** wire previews, the node's type card, the enlarged thumbnail, and tooltips with disabled and stale reasons. Each needs a touch form.
- **Viewer state that is egui-local** (grid format, table sort, text wrap and find, the PDF page, log scale) becomes commands.
- **`px` from the view's size** is new behaviour: today it is 1024 unless `viewer.zoom` raises it.

### 15.4 Found on the way, outside the shells

- **An edit was slow on a large graph.** `node.add` took 3.3 ms at 500 nodes and 30 ms at 2,000, over the 16 ms budget. A profile put it in `plumcraft_session::run::stale_reasons`, which looked at every wire for every node, twice an edit. **Fixed (the maintainer: "Go for stale reasons fix"):** each graph's wires are now indexed once by the node they feed, and once by the node they leave for the downstream walk (`run::downstream` too). `node.add` now takes 1.57 ms at 500 nodes and 12.5 ms at 2,000 (medians of 200, `crates/ffi/tests/measure.rs`, release). The next cost at 2,000 is probably `Graph::is_mapped`, which also scans every wire per node, with a fresh memo for each call. That is a guess from N0(b)'s profile and was not measured.
- **commonkit declines every blob with under 8 GiB free,** on every platform, and its default root on iOS and Android is `/dev/shm`. Most phones will meet the first.
- **Instruments recordings filled the disk once** (about 140 GB of raw traces from saves that stalled). `bench.sh trace` now records briefly and removes its raw trace.

### 15.5 Open after N0

| # | Question | Recommended |
|---|---|---|
| 1 | The iPad at 120 Hz | Optional now that the current rendering is accepted: the maintainer checks Low Power Mode and Limit Frame Rate, then one device run with an Instruments trace, some time before N3's gate |
| 2 | `stale_reasons` | **Done** (15.4): 30 → 12.5 ms at 2,000 nodes. `Graph::is_mapped` has the same pattern, if large graphs need more |
| 3 | commonkit's free-space floor and mobile root | **Decided (the maintainer, 2026-11-19):** "Free space floor is 2GB on mobile; virtimp_com_app already has an answer as to where to put the store". So the root is `<the app's caches>/store`, given by the shell (`Dirs.store`) and set as `COMMONKIT_STORE_ROOT` before the store opens, as that app does. **Where the 2 GB is written: decided, (a) below** |
| 4 | The Gradle wrapper `.jar` under `native/android` | **Decided (the maintainer, 2026-11-19): "commit the gradlew jar"** |
| 5 | The contract's twenty open questions (`docs/shell-interface.md` section 17) | **Decided (the maintainer, 2026-11-19):** "Shell interface Q: 1A; 2A; 3A; 4A; 5A; 6A; 7A; 8A; 9A; 10A; 11B; 12A; 13A; 14A; 15A; 16A; 17A; 18A; 19A; 20A \"Remember this a potentially useful option for later.\"; 21A". All as the draft recommends, except 11: documents share one in-process engine from the start. On 20 (the control channel on the local network): an option to keep for later. 21A: the core writes `{"MIN_FREE_GB": 2}` to the store's `.policy.json` when it makes a store root on iOS or Android and finds none |
| 6 | What next | N1, the core cut, in the slices of section 12; it needs none of the above |

**The floor, and what the native virtimp.com app does (reviewed 2026-11-19).**

- **The root.** Each of its apps passes `store_dir`, `<caches>/store`, when it starts the renderer (`LocalRenderer.swift:185`, `LocalRenderer.kt:113`, `desktop/src/engine.rs:299`); the renderer sets `COMMONKIT_STORE_ROOT` to it unless that is already set (`LocalRenderer/src/lib.rs:79`). PlumCraft's store finds its root the same way (`crates/store/src/lib.rs:64`).
- **No policy file.** That app writes none, so its trees run on commonkit's compiled defaults, the 8 GiB floor included. How it behaves on a phone with less free was not checked.
- **commonkit reads `.policy.json` under the root and never writes it** (`policy.rs`). For each setting the file wins, then `COMMONKIT_STORE_*` in the environment, then the platform default. So defaults are not written into the store's folder today; the file is there for whoever owns the tree to set them.
- **Two places the 2 GB could go.** (a) The core writes `{"MIN_FREE_GB": 2}` to `.policy.json` when it makes a store root on iOS or Android and finds none: no commonkit release, and the file is commonkit's own mechanism for a tree's settings. (b) commonkit's `platform_default` gives 2 GiB on iOS and Android: every app gets it, and it needs commonkit's contract (10.1), its Python reference and a release. They don't conflict.

## 16. N1, slice by slice

- **Slice 1, the core crate (2026-11-19, branch `native/n1-core-crate`).** `plumcraft-editor-core` exists and builds no GUI toolkit; a script checks it in CI. Of section 4.1's table, these moved: `prefs.rs`, `appearance.rs`, `run_anim.rs`, `viewers/yaml.rs`, the theme's tokens as data, `probe.rs`, `diff.rs` (in the core for now, not yet in `plumcraft-media`), the geometry of `wire_route.rs`, the palette's index, the run's text formatters, and `link.rs` with a `Waker`, a closure with no arguments. Rule 1's plain types are `Color`, `Point` and `Rect`; `crates/ui/src/plain.rs` converts them (`ToEgui`, `ToCore`: traits, since Rust forbids a `From` between two foreign types). `plumcraft-ui` keeps its old paths by re-export. **Correcting sections 2.1 and 4.4:** `headless.rs` now uses the core's pump, but it lives in the windowed `plumcraft-editor` crate with `Launcher`, so the windowless commands still link egui until a slice moves them out. Noted for N2: `fruit_slots` builds a set over the whole catalogue for every node every frame; preferences are saved on the UI thread.
- **Slice 2, engines (2026-11-19, branch `native/n1-launcher`).** Starting, hosting, connecting, switching and installing engines are the core's: `engines` (kinds, modes, hosting rules, the packs registry, manifests, the `Launcher`), `switchboard` (what the Engine menu does, as one operation over state the shell lends it, returning the `Effects` the shell applies), `install`, and `headless` (`run`, `check`, `tidy`, `nodes`). The shells keep what draws: the Engine menu, the Attach and Install windows. **Section 6's capabilities exist as `Capabilities {subprocess, craft_apps, python}`,** consulted by the hosting rules, with no `target_os` test anywhere; the Engine menu doesn't yet show refused hostings with their reason (that needs the menu model). **Section 4.4:** the windowless commands are a core module that links no egui, but `plumcraft-editor` is one executable with the window, so that binary still links it. **Section 4.3's process-wide state gains** the packs registry (per process) and the manifests list (per thread). The core now depends on the array, text, coordinator and craft crates, so the array engine's image and PDF decoders are in every core build; a feature per built-in engine would take them out of a phone build. The C ABI's core starts its engine through the `Launcher`; `engine.*` commands sent over it are recorded but nothing performs them until the core has its own thread for engines (slice 3). **For slice 3,** what still calls an engine on the UI thread: sending runs, releases and cancels; a whole engine switch, process start included (10 to 60 s at worst); the first `schema`; `link.state()` every frame; and every `resolve` a viewer, hover or pop-out makes, which for a remote engine is an HTTP fetch.
- **Slice 3, the worker (2026-11-19, branch `native/n1-off-ui-thread`).** Section 4.2's "engine calls go to workers" exists as `EngineWorker`, one thread a document, jobs strictly in order (a cancel after its run, a release after its graph's runs), results held in an inbox the owner drains on its own thread after the same `Waker` its `Link` uses. Sending runs, cancels and releases, and a whole engine switch (stop the old engine, start the new, read its catalogue), run there. The switchboard is three parts: `begin` on the shell's thread, waiting for nothing; `Work` on the worker; `finish` on the shell's thread, with the outcome it always gave. A control request that switches is parked and answered once switched, so the control protocol's "switch before they reply" holds. The C ABI's core uses the same worker, and `engine.*` over it is now performed. A test (`crates/ui/tests/engine_thread.rs`) fails if the thread that owns the editor calls or starts an engine. Longest frame, debug build: a switch to a process engine 190 to 244 ms before, 3 to 6 ms after; with an engine that takes 2 s to start, 2.2 s before, 10 ms after. **Rule 6 is not met in three places yet:** an engine compiled in still connects on the UI thread (about 3 ms; kept so two kittests that assert three frames after a menu pick pass unedited); **startup** still waits for the first catalogue before the window, so section 10's launch row is unmet (`with_link` promises a catalogue on return, and the graph is opened against it); and **`resolve`** is still called from the UI thread for thumbnails, the Watched panel, compare, pop-outs and `plum_blob`, which blocks only for an HTTP engine and needs a "not yet" state the thumbnails lack (it goes with the media crate). One visible change: attaching to an HTTP engine shows "Connecting…" on its button in place of a frozen window, and its kittest waits for that instead of three frames. Engine manifests are per thread, so each switch carries a snapshot of them to the worker.
- **Slice 4, asks (2026-11-19, branch `native/n1-asks`).** The core asks and the shell answers. `asks` is a queue of `{ask, kind, modal, by, params}` (`file.open`, `file.save`, `folder.pick`, `prompt`, `form`, `text`, `clipboard.set`, `clipboard.get`, `share`, `open.external`), each answered or cancelled once, a newer modal ask of a kind withdrawing the older, every answer checked as hostile input. `invoke` is what `EditorApp::invoke` and `try_run` did, as a state machine over a five-method `Host`: New, Open and Quit over unsaved changes, Save and Save As, bundle, insert, themes, Open With, Save Result, path inputs, the review and uploads prompts, command forms. The egui shell answers with what it had (`FileDialogs` is now its adapter for file asks, unchanged for the tests and applications that install fakes). **`by` is new against the contract:** a chain belongs to the person or the script that started it, so a script's file ask is left for the script (the window shows "Waiting for a script") and a person's opens the native dialog at once. **Decision 1 is built:** `ui.invoke` replies at once with the ask in `ui.inspect`'s `asks`, and `ask.answer {ask, answer}` or `{ask, cancel: true}` answers it; an answer whose command switches engines is held until switched. **Decision 19 is built:** the session's app starts go out as `open.external` through a launcher hook on `DocApps`; a failed start now shows in the status bar a moment later instead of as the command's error. The C ABI's `plum_poll` carries `asks` and `asksEnded`. **Not built from section 11 of the contract:** a `doc` field (documents have no ids yet), `commit: true` streamed saves (so section 6's `file.commit` does not exist), stored grants, `startIn`'s fallbacks, a `screenshot` ask, and `share` beyond "not available here"; rename and the naming windows are not yet `text` asks, and Attach and Install stay windows. `Views::ui_asks` is left: it builds its own command's reply, and goes when appearance moves into the core. **A defect of the agent's probe:** its first run named the wrong environment variable and wrote the maintainer's real editor preferences (`dirs.workflow.write`, and two entries in `trusted.json`).
- **Slice 5, debug driving (2026-11-19, branch `native/n1-control`).** Section 9's first two bullets are built: one dispatcher, `plumcraft_editor_core::control`, handles every control method (`command.run`, `command.list`, `graph.get`, `ui.inspect`, `ui.invoke`, `ui.prompt`, `ask.answer`, `ui.waitRun`, parked replies, error shapes) for the egui editor's `--control` port, for `plum_request`, and for a control port a C ABI core now opens itself (config `control: {port, token_file}`, loopback only, the desktop's token rules; `core.control` tells the shell the port; `plum_close` closes it). What only a shell can do is a `Shell` trait of three required methods (`view`, `parked`, `clear_last_error`) and defaults for the rest. The two "reply once switched" implementations of slices 3 and 4 are one (`control::Parked`, `control::step`). **`ui.inspect` is split by owner:** the core builds the document, graph, engine, run, asks, prompt, theme, prefs and catalogue keys; the egui shell adds `popouts`, `parameters`, `editors`, `wirePreview`, `copied`, `selection`, `canvas`, `popups`, `watched`, `groups`, `rename`; a shell with no geometry yet still gives a valid reply. 41 replies captured from the real editor before and after differ in nothing. **`ui.screenshot` over the C ABI is the contract's `screenshot` ask** (2 s to answer); `ui.set` and `ui.audio` say "isn't available from this shell" there, since the core has no canvas view or audio device yet. `plumcraft-automation` gained a `Server` that closes its listener and connections when dropped. **Logging, section 9's third bullet, is not built and there is less to preserve than the plan assumed:** today's editor has no log framework, about 40 `eprintln!` calls, the status bar's messages (now assembled in the core) and a `lastError`; the contract's section 15 (`log.tail`, a ring, a file) is new protocol for its own slice. Over the C ABI only, `ui.invoke` and `ask.answer` now reply with the whole UI state, and a `command.run` error sets `lastError`.
- **Slice 6, media (2026-11-19, branch `native/n1-media`).** `plumcraft-media` (`crates/media`) exists: the decoders moved from `crates/ui` unchanged (picture files, engines' rgba8 renditions, SVG, peaks, clips, spectrograms), a feature for each (`images`, `svg`, `spectrogram`), and the **media service**: a request by value reference answers at once with ready, pending, missing or failed; `resolve` and decoding run on two workers behind a queue of 512; a request nobody has asked for in 120 frames is dropped before it starts, and a new engine link cancels the old one's queue; results sit in a least-recently-used cache with a byte budget (256 MiB by default, 64 in the egui editor, `media.cache_mb` in the C ABI's config). The core depends on media, and its `values` module keeps the service following the engine link. The toolkit gate checks both crates. **Rule 6 now holds for values:** against an engine whose `resolve` takes 300 ms, the editor's thread made 13 of 13 `resolve` calls before and 0 of 11 after, and the longest frame went from 317 to 640 ms down to 4 to 35 ms (the 35 is the in-place spectrogram of a short clip). Thumbnails have a real pending state. What still resolves in place: `doc.open` and `value.open`, whose replies are the copy they made. **C ABI:** `plum_blob` never waits (`PLUM_PENDING`, `PLUM_ERR_UNREADABLE`); `media.get {handle, want, px}`; a `media` list in `plum_poll`. Pixels are the same bytes before and after (202 outputs). **The plan was wrong in one place:** arrays become pictures in the engine SDK (`plumcraft_engine::arrays`), not in the UI, so there was no array mapping to move. **Costs and what is left:** peak memory on 500 nodes with thumbnails is about 10% higher (603 to 610 MB before, 668 to 670 after, debug build) and not explained (the cache budget doesn't move it; the agent's guess is one more copy of each thumbnail); an application's own editors that read a picture once when opened needed a read-ahead to keep working, and on an HTTP engine can still show "gone" for a moment; `probe`, `diff` and `dtype` are still in the core; the image and SVG viewers still start a thread a job; compare, spectrogram, PDF pages and mesh shading are not requests on the service.
- **A bug fix on the way (B27, 2026-11-19).** Linux CI installs the newest stable Rust, and cargo 1.100 builds a test program in `<profile>/build/<package>/<hash>/out/`, not `<profile>/deps/`; `engine_binary` looked beside the program and one folder up from `deps`, so every test that starts an engine as a process failed there. `engines::profile_dir` knows both layouts. With it, slice 3's pull request was the first Linux run in a week with no failing test.
