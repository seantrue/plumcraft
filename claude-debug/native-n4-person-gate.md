---
title: "N4 person gate: the floor re-walked, a launch-as-a-person check, and five decided changes"
date: 2026-11-21
---

# N4 person gate

Branch `native/n4-person-gate` from `origin/feat/native-n0` at `d59d5ba`. Nothing committed. The Rust half of decisions 5 and 26 (the core, egui, the contract's text and their cases) was done by a forked helper in the same worktree and reviewed here; everything Swift, the person check, the floor walk and the documents are this agent's.

## Answer first

- **The floor, re-walked on decision 1:** the Mac 32 met, 2 partly, 51 not met; the iPad and iPhone 28 met, 1 partly, 56 not met. Slice 6 said 77 and 76. Forty-five rows slice 6 called met on the Mac are not met by a launched test, and forty-eight on the iPad (lists below).
- **The person check** (`native/apple/PlumCraftMac/person-check.sh`) passed three runs in a row on the final build, 26 PASS and 2 SEE each, the person's Library and engines folder unchanged each time; it is in the Mac shell's gate (`conformance.sh gate`). It found B80 (every engine hidden in an untitled submenu) on its first run, and B82, B81, B83, B84, B85, B86, B87 since.
- **Decided changes:** 3a pick list, done (core, egui, Mac, iPad); 3b engines folder guard, done; 3c Mac document windows, done for New and Open in a window, with Close, Quit and windowless New and Open left to AppKit as a decision; 3d Recipe, done (Mac and iPad); 3e grid window, done (core, egui, Mac, iPad).

## Part 1: the floor re-walked

A row is met only where a named test launched the real app and went down that path: the Mac's `conformance.sh all` (57 of 57 today, three runs), the iPad simulator's `conformance.sh all` (56 of 56, one skipped, today, on a simulator this agent booted), the iPad UI tests (named, not run today), and the person check (today, Mac only). "The core does it", "the command exists" and pictures do not count.

| | Met | Partly | Not met |
|---|---|---|---|
| Mac | 32 | 2 | 51 |
| iPad and iPhone | 28 | 1 | 56 |

Rows slice 6 called met that are not (Mac, 45): F3 F7 F13 F14 F15 F16 F17 F18 F19 F20 F21 F23 F24 F25 F27 F28 F29 F30 F31 F34 F35 F36 F40 F42 F43 F44 F45 F46 F47 F48 F55 F61 F62 F63 F64 F67 F68 F69 F70 F72 F77 F78 F79 F82 F84; F22 fell from partly to not met.

Rows slice 6 called met that are not (iPad and iPhone, 48): F2 F3 F7 F12 F13 F14 F15 F16 F17 F18 F19 F20 F21 F23 F24 F25 F27 F28 F29 F30 F31 F34 F35 F36 F40 F42 F43 F44 F45 F46 F47 F48 F55 F61 F62 F63 F64 F67 F68 F69 F70 F72 F76 F77 F79 F82 F83 F84; F22 fell from partly to not met.

Claimed on reading and checked today by launching the Mac app (all work): F1, F2 (a process engine killed and started again), F9, F11, F12, F56 (built today), F76, F83. Broken things the walk found are bugs, below; most of the "not met" rows are engine-interface behaviour the core's and engines' own tests cover and no launched app does.

| F | Item | Mac | iPad and iPhone | Evidence (a named test that launched the app, or what was run today) |
|--|--------|-------|-------|------------------------------|
| F1 | Engine connection state | Met | Met | Mac: person check, "Engine › the installed engine: started as a separate process" (the engine cell's words and state) and the F2 step (the engine up again after its process was killed; the cell was not seen to say it was down). iPad: `PlumCraftPad/conformance.sh` waits on the status file's `engine` before any case |
| F2 | Reconnect with backoff | Met (a process engine restarted) | Not met | Mac: person check, "the installed engine's process killed: started again, runs again (F2)": a new process within seconds and the next run computed on it. Not seen: the engine cell saying it was down meanwhile, and the backoff's timing. iPad: engines are in process there; no launched test drops an attached engine |
| F3 | Liveness / health | Not met | Not met | No launched test sees a ping or a health answer; the core's tests only |
| F4 | Catalog fetch | Met | Met | Conformance "a new wire dropped on nothing opens the palette for what takes it" (Mac and iPad simulator); person check "the palette adds a Text and an Upper case node" (Mac, from the installed process engine too) |
| F5 | Version gate | Met, accepting only | Met, accepting only | Every launch's engine handshake (person check: a process engine's hello accepted); no launched test offers an engine a protocol it refuses |
| F6 | Engine identity, capabilities | Met | Met | Conformance "Install Engine… is offered as the platform allows, by the core" (enabled on the Mac, refused with the reason on the iPad) |
| F7 | Catalog hook | Not met | Not met | Claimed on reading (moved to the core in N1); no launched test |
| F8 | NodeDef core | Met | Met | Conformance "a shell's measurements are what the core hits" and every case that draws faces |
| F9 | Wire types | Met | Met | Conformance "a wire dragged from an output to an input is one undo step", "a drop where a wire doesn't fit is refused with the reason"; person check "a wire dragged from Text to Upper case" (Mac) |
| F10 | Families and icons | Met | Met | Conformance "the scene's colours are tokens alone or colours alone…", "the more-button opens the node's menu" |
| F11 | Port widget fields | Met | Met | Conformance "every widget kind is set through its row…"; iPad UI test `testInlineEdits`; person check "a value typed into the Text node's field" (Mac) |
| F12 | Socket modes | Met (checked today) | Not met | Claimed in slice 6 on reading. Person check: a widget input typed into and a socket input wired (Mac). iPad: no launched test tells the two apart |
| F13 | Setting binding | Not met | Not met | No launched test has a `param_ref` port |
| F14 | Variadic inputs | Not met | Not met | No launched test |
| F15 | Choice-dependent sub-ports | Not met | Not met | Conformance sets a choice on Every widget but asserts no port appearing |
| F16 | Path hints | Not met | Not met | The path widget's file ask is not in any launched test |
| F17 | External ports | Not met | Not met | No launched test |
| F18 | Machine-dependent choices | Not met | Not met | No launched test |
| F19 | Node flags | Not met | Not met | No launched test |
| F20 | List typing | Not met | Not met | No launched test runs a list |
| F21 | Wire slices | Not met | Not met | No launched test slices a wire |
| F22 | Custom editors | Not met | Not met | Slice 6 said "partly": the value editor's JSON is drawn by the Mac's and iPad's windows, but no launched test opens one |
| F23 | Codegen metadata | Not met | Not met | Core only |
| F24 | Expansions | Not met | Not met | No launched test expands a node |
| F25 | Style graphs | Not met | Not met | No launched test |
| F26 | Run request | Met | Met | Conformance "a run's state arrives in the scene"; person check Run (Mac, on the process engine) |
| F27 | Targets | Not met | Not met | Run Selected's state is checked (the toolbar case), no launched test runs it |
| F28 | Run identity, generation | Not met | Not met | Core only |
| F29 | Latest wins | Not met | Not met | Core only |
| F30 | Auto-run | Not met | Not met | No launched test turns it on |
| F31 | Optimistic queued | Not met | Not met | No launched test looks at a queued face |
| F32 | Run reply | Met | Met | Conformance "a run's state arrives in the scene"; person check's value (Mac) |
| F33 | Per-node report | Met | Met | The same: each face done or cached |
| F34 | Errors as data | Not met | Not met | No launched test runs a failing node |
| F35 | Failure stands in for a reply | Not met | Not met | The person check kills the engine between runs, not during one |
| F36 | Cancel | Not met | Not met | No launched test cancels a run |
| F37 | Run summary for automation | Met | Met | Every conformance run's `ui.waitRun` over the control port |
| F38 | Status record stream | Met | Met | Conformance "a run's state arrives in the scene" (through the feed) |
| F39 | States emitted | Met, done and cached only | Met, done and cached only | The same case; the other seven states are not seen in a launched test |
| F40 | Reducer rules | Not met | Not met | Core only |
| F41 | Canvas status display | Met | Met | The run case's scene state per face |
| F42 | Per-node timing | Not met | Not met | Slice 6 cited pictures; no launched test asserts the timing text |
| F43 | Parameter wires, bundles | Not met | Not met | No launched test |
| F44 | Provenance | Not met | Not met | Core only |
| F45 | Stale tracking | Not met | Not met | No launched test |
| F46 | Text on nodes | Not met | Not met | The watched text is checked in a pop-out ("a watched text value arrives as text.plain…"); the face's text is not |
| F47 | Thumbnails in flight | Not met | Not met | No launched test |
| F48 | Texture cache | Not met | Not met | The kit's unit tests only |
| F49 | Inspect piggybacks on runs | Met | Met | Conformance "a watched text value arrives as text.plain, and a scalar as scalar"; person check "Run, and the value popped out" (Mac) |
| F50 | Display payload | Met | Met | Conformance's pop-out cases (table, JSON, sound, array, picture, SVG); iPad `testEveryViewerByATap` |
| F51 | View modes | Met | Met | Conformance "an SVG popped out … with its source a viewer away", the JSON case's Tree offered beside YAML |
| F52 | Pixel probe | Met | Met | Conformance "a picture popped out by a person shows a channel, reads its own numbers and asks for detail" |
| F53 | Compare | Partly | Partly | Conformance "Compare pins two values popped out by a person…": pins, sides; the exact difference is not drawn |
| F54 | Tables and scalars | Met | Met | Conformance "a table popped out by a person is sorted and paged by the core", the scalar case |
| F55 | Sheet | Not met | Not met | No launched test opens `viewer.sheet` |
| F56 | Recipe display | Partly (built today) | Not met (built today) | Decision 27: the viewer bar's Recipe. Mac: person check "the pop-out's Recipe" pressed it through accessibility and the popover showed; the text engine sends no recipe, so no recipe's text has been seen in a launched app. iPad: the same SwiftUI bar, built, not launched |
| F57 | Picture window (T16) | Met | Met | Conformance "a value popped out is a window of the core's…"; iPad `testEveryViewerByATap` |
| F58 | Missing data is a state | Met | Met | Conformance "a value read as what it isn't is refused with a reason, and the core still serves" |
| F59 | Curve editor | Not met | Not met | The research software's (N6) |
| F60 | Mask editor | Not met | Not met | The research software's (N6) |
| F61 | List mapping with axes | Not met | Not met | Core only |
| F62 | Vary nodes | Not met | Not met | Core only |
| F63 | Lineage | Not met | Not met | Core only |
| F64 | Save picture with recipe | Not met | Not met | `value.saveAs` raises a save panel; no launched test saves through it (B84) |
| F65 | Export group as plugin | Not met | Not met | The research software's pack |
| F66 | Deliver plugin | Not met | Not met | The research software's pack |
| F67 | Plugin round trip | Not met | Not met | Core only |
| F68 | Arrays never in messages | Not met (exercised, not asserted) | Not met (exercised, not asserted) | The array cases read pages by handle; nothing asserts no array crossed in a message |
| F69 | Content addressing | Not met (exercised, not asserted) | Not met (exercised, not asserted) | Every run uses the store; nothing in a launched test checks a digest |
| F70 | Hostile `.npy` and keys | Not met | Not met | Crate tests only |
| F71 | Engine memo | Met | Met | Conformance "the Cache Report and the graph's Parameters window are the core's…" and the run case (cached faces) |
| F72 | Partial re-run | Not met | Not met | No launched test checks what re-ran |
| F73 | Preview latency path | Met | Met | Every conformance run is on in-process engines over the C ABI |
| F74 | Full resolution on demand | Met | Met | The picture case asks for detail |
| F75 | Pictures as engine-side files | Met | Met | The picture case reads the picture file's own numbers |
| F76 | Workflow JSON is the payload | Met (checked today) | Not met | Person check: saved (the core's `graph.saveAs`), closed, opened again as the Finder opens it, and through the core's File › Open… (Mac). iPad: no launched test reopens a file |
| F77 | Recipes in pictures | Not met | Not met | No launched test |
| F78 | Bundles | Not met | Not met | Slice 6 said met on the Mac; no launched test exports a bundle |
| F79 | Insert graph or group | Not met | Not met | Slice 6 said met; no launched test inserts |
| F80 | Engine-touching commands | Met | Met | Conformance's Install case; person check's Engine menu as AppKit validated it (Mac) |
| F81 | Control channel | Met | Met | Every conformance run; "the control port asks for its token, whatever address it listens on". But it answers for the first document only (B83) |
| F82 | Headless CLI catalog | Not met (not the apps') | Not met (not the apps') | The CLI's, not an app path |
| F83 | Config by environment | Met (checked today) | Not met | Person check: launched with nothing but the environment (store, preferences, engines folder), and refused without a scratch engines folder (decision 7) (Mac) |
| F84 | Single-threaded engines | Not met | Not met | Core only |
| F85 | Local model management | Not met | Not met | The research software's CLI |

## Part 2: the person check

`native/apple/PlumCraftMac/person-check.sh` (with `person-check.py`, `person-check-ax.swift` and the app's `App/PersonSteps.swift`), in the gate as `native/apple/PlumCraftMac/conformance.sh gate` (every case, then the person check on the same build).

**The launch:** the app's own program (not `open`, which could hand the request to a copy the person has running), in a bare environment (HOME, USER, LOGNAME, SHELL, TMPDIR, PATH=/usr/bin:/bin:/usr/sbin:/sbin), with no file, no `--engine`, no `--no-open-panel`. Kept off the person's data by `--scratch-state` and the scratch environment only (store, marks, caches, log, preferences, craft root, test run, engines folder). Driving and looking: `--status-file`, `--control-token-file` (read only) and `--then inbox:DIR`.

**How it drives:** menu items of the real menu bar performed as a pick (validated by their target, then the action sent), a node's context menu built as a secondary click builds it, sheets' controls read and clicked, the palette, the field editor and the canvas's drags (the editing gestures' steps). The save panel and a pop-out's buttons are pressed through accessibility from outside, as an assistive app presses them. The control port is used to read, and for the two things a person can't be imitated safely: writing the file (`graph.saveAs`) and answering Open's panel (B84).

**What it checks** (the final run's lines, in order): the scripted launch refuses without a scratch engines folder and with the person's (decision 7); launch: one untitled graph window (B70); File › New: another, made by the core (decision 20); the Engine menu lists the built-in engines and the installed one by name (B80); the installed engine starts as a separate process; the palette adds a Text and an Upper case node; each node's context menu › Run on › the installed engine; a value typed into the field; a wire dragged; Run, and the value popped out by a click on its pin (`HELLO PERSON`, on the process engine); the engine's process killed by its PID, started again, and the next run computed on it (`ONCE MORE`); the pop-out's Recipe (decision 27); Engine › Attach… and Install Engine… as real sheets, pictures kept, every label beside its own field and wholly shown (from the app's frames, and again from accessibility), Cancel closing each; Install's repository a pick list (decision 5); File › Save As…'s panel seen through accessibility and cancelled; the graph saved and the edited dot cleared; File › Close; opened again as the Finder and Open Recent open a file; closed, and opened through the core's File › Open…; every window closed, then the Dock icon makes a new graph; Quit from the menu, the app gone by itself; the installed engine's process gone with it; the person's Library and engines folder unchanged.

**Three runs on the final build** (`claude-debug/n4-person/mac-gate-1.log` to `-3.log`): 26 PASS, 2 SEE, 0 FAIL each; Library unchanged each. After those runs one step's name was reworded ("shown, started again" to "started again", since the engine cell was never seen to say down); nothing else changed.

**SEE, what it still cannot see or does not do:** saving through the Save panel and using the Open panel (AppKit keeps their folder in the person's defaults, B84); File › Open Recent's items (none in a scripted launch, B50; filling the menu rewrote the person's list on a new build, B81); the app activated and taking the keyboard (forbidden: picks are performed, not clicked; keys are the steps' own); window restoration (ignored in a scripted launch); error alerts (logged instead); the engine cell saying "down" while an engine restarts; what the window server draws (the sheets' pictures are their own drawing); and the iPad.

**What `--scratch-state` changes that a person would see:** no activation; no recent documents (B50); no saved state; pop-out and Settings frames not remembered; errors logged, not alerted; TERM and Quit don't ask about unsaved changes; the Dock click is AppKit's (the check calls the app's reopen); write locations outside scratch refused. None of these is what the check looks at, so no narrower switch was added.

**A scripted ask shows no sheet** (`Asks.swift`: an ask with `by: "script"` is left for the script; the control channel refuses `by`). The check never raises an ask over the port: it picks the menu item, whose dispatch is the person's (`ui.invoke {by: "person"}` from the shell), so the sheet is the one a person sees. No test-only switch.

**The iPad:** no person check. The way there is an XCUITest that launches with `--scratch-state` and a status file only, sees the document browser, creates a graph with its button, and opens the More menu's Engine › Attach… by taps; the iPad's UI tests already drive the canvas that way. Not built: time.

![Engine › Attach…, the sheet's own picture, final build](n4-person/report-shots/attach-sheet.png){width=60%}

![Engine › Install Engine…, final build: the repository a pick list (combo box), the hint whole](n4-person/report-shots/install-sheet.png){width=60%}

![Install Engine… before B82's fix: the hint cut off](n4-person/report-shots/install-sheet-before-b82.png){width=60%}

## Part 3: decided changes

| Decision | Done? | What | Verified by |
|---|---|---|---|
| 5, pick-list field | Done | Core: `FieldKind::Pick {choices, other}`, `install::KNOWN_ENGINES` one list, Install's repository a pick; a closed pick refuses other text. egui's Install window draws the core's form. Mac: combo box (other allowed) or pop-up beside its label, hint as tooltip. iPad: a field with a menu of the choices, typing only when `other` | Conformance case "Install Engine…'s form offers the public engines as a pick list…" (core document, Mac 57 of 57; skipped where install is refused: iPad, kit), hostile-answers test, `kittest_install`, the C ABI's `engines_install`, the person check's Install sheet (combo with DuckDB, OpenCV, Atlas). iPad drawing built, not looked at |
| 7, scratch engines folder | Done | `scratchRefusal` refuses an unset or outside-scratch `PLUMCRAFT_ENGINE_FOLDER`; `conformance.sh` copies the suite's folder into scratch, `bench.sh` gives an empty one | The person check launches both refusals and reads their words; conformance and the person check pass with their scratch folders. `bench.sh` not run |
| 20, Mac adopts `document_windows` | Done for New and Open in a window | `App/DocumentWindows.swift` follows the feed's `window` change; `GraphDocument.follow` moves a window's file when the core opens a file into it; Open's panel is a window of its own | Person check: File › New "made by the core"; File › Open… through the core (into the untouched window); conformance 57 of 57 |
| 20, what can't hold | Left as a decision | Windowless New and Open, Finder and Open Recent opens, closing (NSDocument's Save, Don't Save, Cancel sheet) and Quit (AppKit's review, the system's log-out quit) stay AppKit's | Described in note 24 and the contract |
| 26, grid window in the core | Done | `viewer.grid {cols: next, prev, first, last; col}`, the page's `firstCol`, `endCol`, `colsShown`, `prevCols`, `nextCols`; `viewer.data` refuses `cols`; egui and both Apple apps step through the command | Conformance case "a wide array's numbers are shown 64 columns at a time, in the core's window" on the C ABI's array driver, egui's, the Mac app (57 of 57) and the iPad simulator; session and core unit tests. The apps' grids drawn after stepping were not looked at |
| 27, Recipe in the viewer bar | Done | A Recipe button in both apps' pop-out bars: `viewer.recipe {popout}` in a popover, Copy through `viewer.recipe {copy: true}`, "No recipe" when none | Person check pressed it on the Mac and the popover showed; the text engine sends no recipe, so a recipe's text has not been seen. iPad built, not launched |

**iPad and `document_windows`:** not adopted. Its windows are scenes the document browser makes; adopting it would mean the core's New and Open making documents that the app turns into scenes (`requestSceneSessionActivation`), and the browser's own create and open going to the core. A decision.

## Contract changes

- `docs/shell-interface.md`: the form's `pick` field (`choices`, `other`) and what a shell draws; the grid's window (`viewer.grid {cols, col}`, the page's fields); the Mac declares `platform.document_windows`, and what stays AppKit's.
- `docs/control-protocol.md`: `viewer.grid`'s `col` and `cols`, `viewer.data`'s grid page (`cols` refused; `endCol`, `colsShown`, `prevCols`, `nextCols`), the form ask's field kinds.
- `schema/shell1.json`: regenerated last; unchanged (no message type changed).
- Swift kit: the feed's `window` domain and `Changed {open, from, by}`; `MenuItem.isSection`.

## What Windows must do

- **Pick field:** a ComboBox beside the label, editable when `other`, items the choices' labels (hints as tooltips), answering the chosen choice's `value` or the typed text, preselecting the choice whose value is `values[key]`.
- **Grid:** stop sending `cols` to `viewer.data` (now refused, so today's Windows grid fails), step with `viewer.grid {popout, cols: "prev" | "next"}`, enable the buttons from `prevCols` and `nextCols`, caption from `firstCol`, `endCol` and `cols`.
- **Engine menu:** draw a `look: "section"` item's items inline under its label (B80 was the Apple apps'; check the Windows menu builder for the same).
- **Recipe:** the bar's Recipe, as the Apple apps now have it (another agent's uncommitted work already has one).

## Bugs

| Bug | State | What |
|---|---|---|
| B80 | Fixed | The Apple apps' Engine menu put every engine in an untitled submenu (`look: section` drawn as a submenu) |
| B81 | Open | Filling File › Open Recent in a scripted launch of a new build rewrote the person's recent-documents list file |
| B82 | Fixed | The Install form cut its hint off |
| B83 | Open | The control channel acts on the first document only and ignores `doc` silently |
| B84 | Open | A scripted launch's Save or Open panel changes the person's defaults or saves into their folders |
| B85 | Open | Two crashes in Swift's main-actor check while the check drove a panel from inside a task (the check's defect, probably) |
| B86 | Open | Run on names engines by id |
| B87 | Open | A manifest the core can't read is skipped silently |
| B88 | Open | The more-button conformance case failed once on the unchanged dev branch |
| B40 | Seen again | A neighbouring kittest failed once in the full run |

## Defects introduced and caught

- **A graph saved into the maintainer's folder:** the check's fifth run pressed Save in a save panel it had told, in process, to show scratch; the panel (out of process) showed the person's last folder and saved `Untitled.json` there (the `workspace` folder in the home folder, 643 bytes, 21:13). Left in place: it is outside the worktree.
- **The maintainer's Plummer defaults changed:** the sixth run started the save panel in scratch (`prepareSavePanel`), and AppKit wrote that folder as `NSOSPLastRootDirectory` in `com.swapwizard.plummer.mac`. The code was taken out; the value was not restored (only a truncated copy of the old one was kept).
- **The person's recent-documents list file rewritten** four times on new builds while the check read File › Open Recent (B81); the check no longer opens it.
- **Two app crashes** (B85) while the check called the save panel from inside a task; the check's steps now run from the run loop.
- **A debug copy of the app left running** for about five minutes after a kill command was refused by the sandbox; found by the Library check's note, killed by its PID.
- **A context-menu pick that did nothing:** the menu was released before its action ran; kept alive now.
- **The kit's conformance test** expected only array cases to be skipped; the new install case is skipped there too.

## Tests and builds

| What | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --all-features --no-fail-fast` (scratch env) | 1,580 of 1,581, 17 skipped; the one failure `hidden_params_match_the_baseline`, known |
| Hash and core-toolkit gates | pass |
| Python (scratch env) | 225 passed; ruff clean |
| xcframework | rebuilt, macOS (universal), iOS, iOS simulator |
| `swift test` | 94 tests in 30 suites pass |
| Mac `conformance.sh all` | 57 of 57, 0 skipped, three runs; Library unchanged each |
| Person check | three runs, 26 PASS, 2 SEE, 0 FAIL each; Library and engines folder unchanged each |
| iPad simulator `conformance.sh all` (iPad Pro 11-inch (M5), booted and shut down by this agent) | 56 of 56, 1 skipped (the install case) |
| iPad UI tests | not run |
| Mac Release build (clean) | builds; one Xcode notice ("Metadata extraction skipped, no AppIntents.framework dependency found"), no compiler warning |
| iOS Release, device SDK, unsigned | builds; the same notice, no compiler warning |
| egui's tests | in the nextest run (all pass but the known one) |
| Private strings | no match in tracked files; the new untracked files checked against the same patterns, no match |

## Before and after

- `~/.config/plumcraft/engines` and `~/.config/plumcraft`: unchanged.
- The Mac Library check against the session's first snapshot: two differences, both this agent's (above): `NSOSPLastRootDirectory` in the Plummer defaults, and the recent-documents list file's properties. Every person check and conformance run since those fixes left it unchanged.
- No Plummer process of this agent's running; the simulator this agent booted is shut down; the iPad Pro 13-inch simulator that was booted before is untouched.
- Disk: about 58 GB free at the start.

## Decisions for the maintainer

1. The Mac's Close and Quit: keep AppKit's (NSDocument's sheet, AppKit's review, the system's quit), or bridge them to the core (`document.close {by: person}`'s prompt replacing the Mac's sheet; `app.quit` from `applicationShouldTerminate` answered later)?
2. The iPad and `platform.document_windows`: adopt (core-made documents as scenes), or keep the document browser's?
3. B84: let scripted launches run under a bundle id of their own, so the person check can save and open through the panels, or keep the panels out of the check?
4. B83: should the control channel take a request's `doc`, or refuse it?
5. B81: should a scripted launch leave Open Recent's menu empty without asking the document controller to fill it?
6. The stray `Untitled.json` in the home folder's `workspace` folder and the changed `NSOSPLastRootDirectory`: remove and reset them yourself, or tell an agent it may.
7. The rows not met because only the core's tests cover them (the engine interface: F13 to F25, F27 to F31, F34 to F36, F40, F43 to F48, F61 to F72, F84): add a launched-app engine half (a test engine in the app, run through the conformance runner), or accept the core's tests for them?
8. B86 and B87: fix now (small, in the core)?
