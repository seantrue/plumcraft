# 28. The status line: what egui's bar shows, and the same line for every shell

*2026-11-25. Research note for the maintainer's request:* "The information in the original egui status line was timely, centralized, and includes more than I can easily see now. Analyze the original egui status and provide it as an option status line for the editor."

## 1. What the egui editor's status bar shows

The bar is a bottom panel (`crates/ui/src/lib.rs`, `status_bar`), one row, read left to right. Since plumcraft #86 the engine, document and run cells are two lines high: a main line and a smaller, dimmer detail line under it (`crates/ui/src/status_cell.rs`). Cells are separated by thin rules. It is drawn every frame, so everything in it is current.

| # | Item | When it shows | Look | Click | Tooltip | Source in the core |
|---|---|---|---|---|---|---|
| 1 | Engine dot | Always | Green connected, amber connecting or loading the catalogue, red otherwise | — | — | `Document.engine` (`EngineState`) |
| 2 | Engine name, short ("Text + Array 0.7.0") | Always | Plain; with a warning sign in the warning colour when an engine in use has a manifest warning (decision 17) | — | The long label (engines, mode, nodes); the warning first when warned | `switchboard::engine_short_label`, `engine_label`, `engine_warning` |
| 3 | Engine status (detail line) | When the connected engine's catalogue says one (`texts.engine_status`) | Dim for info, warning and error colours by its tone; cut at 240 points | — | The whole text | `control::engine_status` (an application's engine adds or omits it through its catalogue texts) |
| 4 | Installing an engine | While an install runs | Spinner, "installing owner/repo…" | — | uv's latest line | `Document.installing`, `engines.install_status` |
| 5 | Document name, with "•" when edited | Always | Plain | — | Its path ("(unsaved changes)"), or "not saved yet" | `control::document_name`, `Session::is_dirty`, `path` |
| 6 | Graph size (detail line) | Always | Dim, "4 nodes, 3 wires" | — | — | `Session::graph()` |
| 7 | Hash | Always | Dim, "hash b3 · policy file" | — | "Every handle this editor makes and reads is b3 (…). Engines must use the same." | `plumcraft_proto::handle::Algo` |
| 8 | Run, while running | During a run | Spinner, "running: 3 of 8 nodes", a Cancel button; detail "2.4 s" | Cancel: `run.cancel` | What Cancel does | `RunState::progress`, `elapsed` |
| 9 | Run, after a run | After the first run | Dim "last run 1.2 s"; detail "engine 0.9 s · cached 5 of 8 · saved 3.1 s", and in the warning colour "store refused 2 writes" or "disk low: 3 GB free" | The cache part: `run.cacheReport {open: true}` | Where the hits came from, the store, the session's runs; why writes were refused | `RunState::last_wall`, `last_reply.elapsed_ms`, `cache` |
| 10 | Not run yet | Before any run | Dim | — | — | `RunState::last_wall` |
| 11 | Auto-run | Always | A toggle, lit when on | `run.auto {on}` | "Run 150 ms after every edit" | `RunState::auto` |
| 12 | Saving | While a save is pending | Spinner, "saving folder/file…" | — | The whole path | `Session::save.pending` |
| 13 | Insert a converter | After a refused wire offers one | A button "Insert <label>" | `wire.convert` | What it places | `Document.convert_offer` |
| 14 | Messages | When there are any | Chips, most serious first, each in its tone, cut to the width left; "+N more" | Opens the Messages window (`view.messages`) | Each message in full, with its detail | `control::status_messages`: the run's errors ("2 errors", in which nodes), the run's own error, the application's segment, a save's outcome, the last error |

Persistent: 1 to 3, 5 to 7, 9 to 11 (always present in some form). Transient: 4, 8, 12, 13 (while their condition lasts). Messages (14) stay until what they report changes: the last error until a command a person or script invokes clears it (the core's rule), a save's outcome until the next save, the run's errors until the next run.

## 2. What the native shells show of it today

| Item | Mac | iPad, iPhone | Windows |
|---|---|---|---|
| Engine dot and name | The toolbar's engine label | The toolbar's dot and label (no label in compact width) | A status-bar cell (dot and label) |
| Engine status (catalogue text) | No | No | No |
| Installing | The toolbar label says "Installing an engine…" | No | No |
| Document name and edited | The window title and its edited dot | The title | The window title, "*" |
| Graph size | No | No | No |
| Hash | No | No | No |
| Run progress and Cancel | Faces show their states; Cancel in the toolbar and menu; no count or time | Same, no count or time | No count or time |
| Last run's time, engine time, cache, store refusals, disk low | No | No | No |
| Auto-run | Run menu item | Run menu | Run menu |
| Saving | No | No | No |
| Insert a converter | No (the wire's context menu offers converters) | Same | Same |
| Messages | A banner for 5 seconds; the Messages window from the menu | A banner; the Messages sheet | The last error in the status line (one line, whole on hover) |
| Zoom | The window's subtitle | No | A status-bar cell |

## 3. What a person loses in the native shells, concretely

**Timely:** what the editor is doing now and just did: the run's count and time while it runs, and how long the last run took, how much came from the cache, and the engine's own time; a save in progress; an engine install's progress (the Mac only says "Installing an engine…"). The native shells show a run only face by face.

**Centralized:** one place to look. Today the engine is in the toolbar, the document in the title, errors in a banner that goes after 5 seconds, the zoom in the subtitle, Auto-run in a menu. After the banner, a message is only in the Messages window, which a person has to know to open.

**More than I can easily see now:** things no native shell shows anywhere: the graph's size, the hash and where it came from, the engine's own status text (an application's engine uses it), the last run's engine time and cache hits, the store refusing writes or the disk running low, a pending save, the converter offer, and the run's error count with the nodes in error.

## 4. The design built (decisions as assumptions for the maintainer)

- **One model, the core's.** `plumcraft_editor_core::status_line::model` gives the line as `{on, segments}`, each segment `{id, role, side, priority, text, detail?, detailTone?, tone, dot?, busy?, on?, command?: {id, params, label?}, tooltip?}`: what the egui bar shows, in its order, with the click it runs (`run.cancel`, `run.cacheReport`, `run.auto`, `wire.convert`, `view.messages`) and its tooltip. No shell composes status text. It is in `ui.inspect` (`statusLine`) and in the feed's `document` domain (`statusLine`), so it changes when what it says does (the running time is said to the tenth of a second the egui bar shows).
- **Added beside egui's items:** the selection ("2 selected", the count of selected nodes and groups), since a native shell has no other count of it. The zoom stays where each shell has it (the Mac's subtitle, the Windows bar's own cell): it is the shell's viewport, not the core's.
- **An option.** `view.statusLine {on?}` (View › Status Line, with a check mark), kept in preferences (`statusLine`, unset meaning on). **Default: on**, since the maintainer asked for the information back and the line is how it comes back. On an iPhone (compact width) the shell shows only priority 1 (the engine, the run, the newest message), so a small screen keeps the timely part in one short row. A regular width leaves out priority 3, then 2, as the width runs out (the Mac and iPad measure; Windows, not launched, uses widths of 1100 and 760 points). Alternative: off by default, on by the menu.
- **The last message stays.** The messages segment shows the newest message after its banner has gone, until what it reports changes, as egui's does; a click opens the Messages window.
- **An application's own engine** adds to the line only through what it already says: its catalogue's engine status text and tone (item 3) and its node states; nothing private is named.
- **Shells.** Mac: a bottom bar in the document window (the title bar's material, a rule above), the canvas above it and the banner above both. iPad and iPhone: a bar on the keyboard layout guide (above the home indicator, rising with a keyboard), its material down to the edge, the banner above it. Windows: its existing status bar's engine cell and last-error text replaced by the model's segments; the zoom cell stays. egui: its own bar, hidden by the same option (section 5).

No real fork turned up: the choices above are of wording, defaults and placement.

## 5. egui

The egui bar is not redrawn from the model: it is about 150 lines of egui widgets (two-line cells, message chips cut to the width left with "+N more", the cache cell's own buttons, the Messages window it opens), and drawing it from the segments would change how it looks for no gain to a person. What changed: View › Status Line hides it too (the same command and preference), and its hash words come from the core's `status_line::hash_label`. The model was written from it item by item, so the two say the same things; what differs:

| egui bar | The model |
|---|---|
| The cache cell's "cached 5 of 8 · saved 3.1 s" is its own button | Part of the run segment's detail; the run segment's click opens the Cache Report |
| Messages as chips with "+N more" | One segment per message (at most 16), most serious first |
| No selection count | `selection` segment |
| The application's segment from its own hook | The same, where a shell passes it (egui's `ui.inspect`); the C ABI's shells have none |

## 6. Not built

- A Messages window opened from the line on the Windows shell is the core's `view.messages` like any other; it was built, not launched.
- The line's running time updates when the feed's document domain is noted, which is on every core wake (each node event of a run), not on a clock.
