# N5 gate: the launch as a person would, and the Windows floor re-walked

*2026-11-21, branch `native/n5-gate` (from the dev branch at `1a416e5`), uncommitted. In progress: the Windows test VM stopped answering.*

## Answer first

The person check exists and found two real defects that every earlier gate missed. The conformance suite reads the core's model over the control port, not what the app draws, so it could not see them.

- **B90:** the Engine menu listed no engine. Fixed, and a launched app then listed every engine.
- **B91:** a separate-process engine never connected, because no engine program ships beside the app. The build fix is written but not run.

Under the strict rule, the Windows floor stands at:

- 54 met;
- 4 partly;
- 5 not met;
- 0 broken: F6 and F80;
- 23 claimed on reading alone and not checked.

The maintainer restarted the VM. Later it stopped answering a second time: SSH accepted connections but sent nothing, then ping stopped too. So most of the gate is not taken.

## Part A

### A1. The launch as a person would

`native/windows/person-check.ps1` starts `Plummer.exe --root <scratch>` and nothing else. It then works only through real input (`SendInput`) and UI Automation, and takes a screenshot after each step. It prints PASS or FAIL for each step with what it saw.

Two folders are redirected through the environment:

- the engines folder (`PLUMCRAFT_ENGINE_FOLDER`), which holds a copy of the suite's probe manifest;
- `TEMP`.

So that the check (and a screen reader) can see the canvas, the canvas now has a UI Automation peer (`CanvasPeer.cs`, `CompositionCanvas.Access.cs`). Each face is a list item named by its title, with its state and footer lines as item status. Its pins are its children.

**Run 1, before the Settings step:**

- passed: the window, File › New Graph, closing that window, Attach…, and Install Engine… disabled with its reason;
- added a node from the palette by typing and pressing Enter. "Text" gave Draw Text, the first in category order;
- clicked Run, and the face showed the error as data;
- opened the value's window from the output pin's "Watch text";
- saved to a file;
- failed the Engine menu step: its dump showed the engines in an unlabelled submenu. That is **B90**, fixed.

**Run 2, after the B90 fix:**

| Step | Result | What it saw |
|---|---|---|
| window | PASS | one window, "Untitled - Plummer" |
| file-new | PASS | File › New Graph gave a second window |
| close-new | PASS | its close button closed it |
| engine-menu | PASS | Text · Rust · in-process (mode 1); Audio; Array; Conformance Probe; the crafts disabled |
| settings-auto-off | PASS | Settings › Engines › Pick Engines Automatically turned Off |
| engine-separate-process | FAIL (reported PASS by a bug in the check, now corrected) | chose Rust · process (mode 2); the status line stayed "process (mode 2), not connected": **B91** |
| attach-form | PASS | Address and Token, each label at its own field's left edge |
| install-disabled | PASS | "This device can't install engines: it has no Python", given to UI Automation as help text; no tooltip showed to the check |
| palette-add | FAIL | the pick waited for the Text engine, which never connected (B91) |
| wire, run, pop-out | FAIL | follow from the above |
| save-as | PASS | the file was written, and the title took its name |
| close | FAIL | the new window covered the saved window's close button; the check now brings a window forward by clicking its own title bar |
| reopen | FAIL | follows from close |
| quit | PASS | File › Quit ended the app |

**Changed since run 2, not yet run:**

- "not connected" no longer counts as connected;
- a palette item is picked by clicking it;
- a window is brought forward by clicking its own title bar;
- reopening needs the close to have happened first;
- Install Engine… opens and cancels the core's form when it is enabled.

Install Engine… is enabled now that Python is declared when uv is found. The disabled reason can only be seen on a machine without uv.

**What it covers:**

- the menus;
- Settings;
- the core's forms;
- the palette;
- a drag between pins;
- Run;
- a value's window;
- the save and open pickers;
- closing a window, and quitting.

**What it cannot see:**

- how things look (the screenshots are for a person);
- sound;
- smoothness;
- wires (the canvas peer doesn't list them);
- anything else UI Automation doesn't expose.

### A2 to A4

Not taken, because the VM stopped answering:

- the screenshot retakes;
- `--bench-panels`, three runs;
- the conformance runs on the final build.

## Part B: the Windows floor re-walked

A row is met only when a named test, case or script launched the real app and exercised that path. The evidence is named as follows:

- **PC:** the person check's runs today.
- **SC n:** suite case n, run against the launched Windows app over its control port in slice 4's gate. These drove the app's core, not its UI.

**Wrongly claimed before:**

- **F6 and F80** were "Met", but the Engine menu showed no engine (B90).
- **F80** also offered hostings that could not start (B91).
- **23 rows** rested on "Core" or on a menu item existing; the old walk counted them as met without a launch.

| F | Item | Now | Evidence |
|---|---|---|---|
| F1 | Engine connection state | Met | PC: the status line read "engine: Text 0.7.0 · Rust · mode 1 · in-process · connected", and "process (mode 2), not connected" for the hosting B91 broke |
| F2 | Reconnect with backoff | Claimed on reading alone, not checked | Nothing launched the app and broke an engine's link |
| F3 | Liveness / health | Claimed on reading alone, not checked | The suite's port driver pings the core, not an engine's health |
| F4 | Catalog fetch | Met | PC: the palette searched and listed kinds (run 1 added Draw Text; run 2 listed Text first); the status line's "47 nodes" |
| F5 | Version gate | Claimed on reading alone, not checked | The core's; the hostile sweep is not a launched app |
| F6 | Engine identity, capabilities | Met | PC run 3 (2026-11-22): identity and version on the status line; the Engine menu lists every engine inline (B90 fixed; screenshot engine-warning, looked at); Install Engine… enabled where uv is found |
| F7 | Catalog hook | Claimed on reading alone, not checked | No case or check |
| F8 | NodeDef core | Met | SC 1 "a shell's measurements are what the core hits"; PC read the Draw Text and Upper case faces and pins through the canvas peer |
| F9 | Wire types | Met | SC 7 "a wire dragged from an output to an input is one undo step", SC 11 "a drop where a wire doesn't fit is refused with the reason" |
| F10 | Families and icons | Met | PC screenshot 10 (the Draw Text face with its family colour and icon), looked at |
| F11 | Port widget fields | Met | SC 26 "every widget kind is set through its row…"; PC screenshot 10 shows the fields |
| F12 | Socket modes | Met | SC 31 "the parameters inspector lists every input…" |
| F13 | Setting binding | Met | SC 31 |
| F14 | Variadic inputs | Met | SC (2026-11-22, the launched Windows app, ARM64 and x64, the test engine installed): "ports: a variadic input, a choice's sub-port, and a choice this machine can't take" |
| F15 | Choice-dependent sub-ports | Met | SC (2026-11-22, the launched Windows app, ARM64 and x64, the test engine installed): the same case (a choice's sub-port) |
| F16 | Path hints | Met | Person check step 20 "path-browse" (2026-11-23, the launched ARM64 app, the test engine installed): Read Text File's Browse opens the system dialog "Choose a File" in the value's folder with Text (*.txt) as its filter (read through UI Automation, pictured, cancelled); Browse with the field open closes it without its text; SC "ports: a path input's Browse asks for a file by its hints, and the answer sets the value" (the scripted answer, one undo step) on ARM64 and x64; SC "a path input" sets and the engine reads it |
| F17 | External ports | Met | SC (2026-11-22, the launched Windows app, ARM64 and x64, the test engine installed): "ports: a setting bound to a bundle, and external ports on a collapsed group" |
| F18 | Machine-dependent choices | Met | SC (2026-11-22, the launched Windows app, ARM64 and x64, the test engine installed): the same case (a choice this machine can't take, refused with its reason) |
| F19 | Node flags | Claimed on reading alone, not checked | No case or check |
| F20 | List typing | Met | SC (2026-11-22, the launched Windows app, ARM64 and x64, the test engine installed): "ports: a list, a wire's slice of it, and a path input" |
| F21 | Wire slices | Met | SC (2026-11-22, the launched Windows app, ARM64 and x64, the test engine installed): the same case (a wire's slice) |
| F22 | Custom editors | Partly | Reading: the value editor window was never opened in a launched app (no engine here names an editor) |
| F23 | Codegen metadata | Claimed on reading alone, not checked | No case or check |
| F24 | Expansions | Claimed on reading alone, not checked | No case or check |
| F25 | Style graphs | Claimed on reading alone, not checked | No case or check |
| F26 | Run request | Met | PC run 3: Run on the toolbar ran Upper case to "HELLO PERSON" on the separate-process engine; SC 21 |
| F27 | Targets | Met | g-floor-checks.ps1 (2026-11-22, the launched app over its port): Run Selected ran the selected node and left the other "not run" |
| F28 | Run identity, generation | Met | SC 21 |
| F29 | Latest wins | Met | g-floor-checks.ps1 (2026-11-22, the launched app over its port): two edits and runs in a row; Upper case shows TWO, not ONE |
| F30 | Auto-run | Met | g-floor-checks.ps1 (2026-11-22, the launched app over its port): Auto-run on, an edit ran by itself (Upper case: AUTO) |
| F31 | Optimistic queued | Met | SC 21 (the run's states, queued first, in the scene the app draws) |
| F32 | Run reply | Met | SC 21, SC 22 "a watched text value arrives as text.plain…" |
| F33 | Per-node report | Met | PC run 1: Upper case's face read "error · config error: input "text" has no value"; SC 41 |
| F34 | Errors as data | Met | PC run 1 (as F33); SC 49 "the Messages window is the core's…" |
| F35 | Failure stands in for a reply | Met | g-floor-checks.ps1 (2026-11-22, the launched app over its port): a Fail node's run shows "op error: this node always fails" on its face |
| F36 | Cancel | Met | g-floor-checks.ps1 (2026-11-22, the launched app over its port): a Slow node running ("running 24%") cancelled by run.cancel, its state "cancelled" |
| F37 | Run summary for automation | Met | SC 21 and every run case over the app's port |
| F38 | Status record stream | Met | SC 21 |
| F39 | States emitted | Met | SC 21 |
| F40 | Reducer rules | Claimed on reading alone, not checked | The core's |
| F41 | Canvas status display | Met | PC screenshot 10: "not run" in the face's header, looked at; PC read the faces' states |
| F42 | Per-node timing | Met | Slice 4's scene screenshots (`cached · 0.21 ms` on faces, looked at) |
| F43 | Parameter wires, bundles | Met | SC (2026-11-22, the launched Windows app, ARM64 and x64, the test engine installed): the same case (a setting bound to a bundle) |
| F44 | Provenance | Claimed on reading alone, not checked | No case or check |
| F45 | Stale tracking | Met | SC (2026-11-22, the launched Windows app, ARM64 and x64, the test engine installed): "the engine's run changed while it goes: stale, cancel, latest wins, a partial re-run, auto-run" |
| F46 | Text on nodes | Met | SC 41 "a number shows the same text everywhere…" |
| F47 | Thumbnails in flight | Met | SC 23 "a watched picture arrives as picture"; slice 4's screenshots of picture previews and waveforms, looked at |
| F48 | Texture cache | Claimed on reading alone, not checked | Pictures are cached in the app; nothing checks a second ask is served from it |
| F49 | Inspect piggybacks on runs | Met | PC run 3: Watch text from the output pin's menu opened the value's window showing HELLO PERSON; SC 22 |
| F50 | Display payload | Met | SC 35 "a value popped out is a window of the core's…" |
| F51 | View modes | Met | SC 48 "a value read as what it isn't is refused…", SC 42 to 47 |
| F52 | Pixel probe | Met | SC 46 "a picture popped out by a person shows a channel, reads its own numbers…" |
| F53 | Compare | Partly | SC 52 "Compare pins two values…"; the exact difference is the egui editor's |
| F54 | Tables and scalars | Met | SC 42 "a table popped out by a person is sorted and paged by the core", SC 22 |
| F55 | Sheet | Claimed on reading alone, not checked | The items viewer was never shown with content in a launched app; the pop-out stepper reads the sheet's labels (step 18), but the sheet view itself (viewer.sheet) was not opened |
| F56 | Recipe display | Met | Person check step 16 "recipe" (2026-11-22, the launched ARM64 app, the test engine installed): Traced's pop-out offers Recipe and its flyout reads {"params": {"seed": "plum"}, "style": "traced"}; Result's pop-out has no Recipe (screenshots recipe-traced and recipe-none looked at); SC "ports: a value with a recipe offers Recipe, and one without doesn't" on ARM64 and x64 |
| F57 | Picture window (T16) | Met | SC 35, SC 46; PC run 1's value window |
| F58 | Missing data is a state | Met | SC 48 |
| F59 | Curve editor | Not met | The research software's editor (N6) |
| F60 | Mask editor | Not met | The research software's editor (N6) |
| F61 | List mapping with axes | Partly | Person check step 18 "list-names" (2026-11-23, the launched ARM64 app, the test engine installed): Items' four words wired into Result make it run once a word; its pop-out steps by the labels the mapping gives ("text a · 1 of 4", a pick list of all four, "text c · 3 of 4" chosen from it). One axis only: two lists crossed (axes) not launched |
| F62 | Vary nodes | Claimed on reading alone, not checked | No case or check |
| F63 | Lineage | Claimed on reading alone, not checked | No case or check |
| F64 | Save picture with recipe | Partly | SC (2026-11-22, the launched Windows app, ARM64 and x64, the test engine installed): "ports: Save Result writes a result node's value in each of its formats" (the file picker's kinds, the options form, both files read back); the person check's step 15 from the footer; no recipe, since no engine here tracks lineage; person check step 17 "save-shape" saves a 4 by 1000 array from its footer: the core's question with its three labelled buttons (picture looked at), then an .npy and a PNG read back. A picture saved with its recipe in it is still not checked |
| F65 | Export group as plugin | Not met | The research software's pack |
| F66 | Deliver plugin | Not met | As F65 |
| F67 | Plugin round trip | Claimed on reading alone, not checked | No case or check |
| F68 | Arrays never in messages | Met | SC 45 "an array popped out by a person is a picture…" (lent by handle, read through `media.get`) |
| F69 | Content addressing | Claimed on reading alone, not checked | The core's |
| F70 | Hostile `.npy` and keys | Claimed on reading alone, not checked | Rust tests, not a launched app |
| F71 | Engine memo | Met | SC 50 "the Cache Report and the graph's Parameters window are the core's…" |
| F72 | Partial re-run | Met | SC (2026-11-22, the launched Windows app, ARM64 and x64, the test engine installed): the same case (a partial re-run) |
| F73 | Preview latency path | Met | SC 21 to 23 with the in-process engines |
| F74 | Full resolution on demand | Met | SC 46 ("…and asks for detail") |
| F75 | Pictures as engine-side files | Claimed on reading alone, not checked | The core's |
| F76 | Workflow JSON is the payload | Met | PC run 3: Save As wrote the graph's JSON, the window closed, File › Open reopened it with both nodes; SC 39 |
| F77 | Recipes in pictures | Claimed on reading alone, not checked | No case or check |
| F78 | Bundles | Claimed on reading alone, not checked | Export Bundle… was disabled on the empty graph the check had; never used |
| F79 | Insert graph or group | Claimed on reading alone, not checked | Never used in a launched app |
| F80 | Engine-touching commands | Met | PC run 3: Attach…'s form, Install Engine…'s pick-list form, Text hosted as a separate process and connected (B91 fixed); the copied engine marked with the warning glyph (screenshot) and, chosen by clicks, failing at once with its stderr in the floating banner (B72; screenshot b72-person) |
| F81 | Control channel | Met | SC: the whole suite over the app's port |
| F82 | Headless CLI catalog | Claimed on reading alone, not checked | The CLI's, not the app's |
| F83 | Config by environment | Met | PC: `--root` alone, and the engines folder from `PLUMCRAFT_ENGINE_FOLDER` (the probe listed) |
| F84 | Single-threaded engines | Claimed on reading alone, not checked | The core's |
| F85 | Local model management | Not met | The research software's CLI |
| S20 | Sound heard by a person (decision 20) | Met | The maintainer, 2026-11-21, through the VM's speakers: the Mandelbrot sound "Sound works. Not a tone, however. More like a sick bumblebee." (an 80 Hz buzz, as that graph makes) and the 440 Hz triangle "440 works."; the WAVE matched the core's lent samples exactly (g-wav.py) |

## Part C

| Decision | State | Verified by |
|---|---|---|
| Install: Python declared when uv is found | Written (`Uv.Find` in `ShellConfig.cs`) | Not compiled or run. uv is on the VM's PATH; the DuckDB install was not reached |
| Wires: take the tiles now | Written and compiled (`CompositionCanvas.Tiles.cs`, `--wire-layers`, `tiles_drawn` and `tile_mb` in the bench) | Not run: no bench, no pixel comparison, not looked at |
| The banner floats over the canvas | Written and compiled (an opaque card over the canvas's top edge) | Not looked at |
| B52: make the test wait | Written (`scene.get` before the 500 requests) | Not run |
| VM display timeout: never | Done: `powercfg /change monitor-timeout-ac 0` and `monitor-timeout-dc 0` in the Balanced scheme. Before: 300 s on AC, 180 s on DC. Sleep was left as it was (never on AC, 600 s on DC) | Read back with `powercfg /query` |
| Inspector: keep the docked pane | Kept | Nothing to change |
| Geometry in runners: the default for every runner | Already so: a measured driver reports the suite's geometry; `--own-geometry` opts out (C# and Swift runners) | Reading; nothing changed |
| Sound: accept after a person hears it | Nothing built; the maintainer to listen | None |
| B54: open until repeated runs are done | No clean runs added today | None |
| B56: undecided | Left open | None |
| Not mine | Not built against | See below |

**What Windows will need when the other work lands** (from the coordinator's heads-up):

- **The numbers grid:** step columns through `viewer.grid {cols}`, and enable the buttons from `prevCols` and `nextCols`, since `viewer.data` will refuse `cols`.
- **The pick-list form field:** a ComboBox beside its label, editable when `other` is set.
- **Recipe on Apple:** nothing; Windows has it.
- **The Mac's document windows:** nothing yet.
- **The suite's 57 cases:** the grid and install cases need the two items above.
- **B72:** to be checked on Windows.

## Defects introduced and caught

- The person check first passed the separate-process step on "not connected", because the pattern "connected" matched it. Corrected.
- Its first close and reopen "passed" while the window had never closed. Corrected.
- Found with B90 and fixed: the old in-place menu update paired items with their views in a different order from the one they were made in.

## Unverified

These were synced but not compiled before the VM went down:

- `Uv.Find`;
- the engine programs in the app's build;
- the person check's later changes;
- B52's test.

Run on the VM or the Mac today: none of the gate.

- `dotnet test`: not run.
- The C# and Swift runners: not run.
- The Rust gates: not needed, since no Rust changed.

## Second session (the VM back with 10 GB)

### Timeline

| Time (Mac) | Step | Available MB |
|---|---|---|
| 22:14 | VM checked: console active, no Plummer | 7176 |
| 22:15 | synced, app built (uv detection compiled) | 7115 |
| 22:16 | Mandelbrot sound set up, visible and unmuted (PID 11048) | 7111 |
| 22:17 | that app quit by its PID after the maintainer listened | 7097 |
| 22:20 | dev branch merged (`e36ee7b`, fast-forward, docs re-applied), VM checkout moved to it, synced | 7126 |
| 22:20 to 22:21 | core DLL and the text and array engine programs built (66 s), app built | 6857 |
| 22:21 to 22:31 | tone set-ups, WAVE dumps, the autoplay tries (one app at a time, each quit by its PID) | 7004 to 7093 |
| 22:31 | app rebuilt with logging on Play | 7056 |
| 22:34 | last app quit by its PID; a stale `plummer-person-check` task (left by the earlier failure) removed | 7032 |
| 22:35 | `dotnet test` started; SSH timed out at 22:36: the VM stopped answering, the third time today | n/a |

### Sound, decision 20

- **Graph 1:** a Mandelbrot picture (300 × 400) read row by row at 24 kHz: 5.00 s, one channel. It should be a click train at 80 Hz (one row every 300 samples), not a tone. The maintainer: "Sound works. Not a tone, however. More like a sick bumblebee."
- **Graph 2:** a 440 Hz triangle wave made in-process with the array engine, which has no sine node. Each 100-sample row is the minimum of x and 100 − x, played at 44 kHz: 4.55 s, peak 0.3. The maintainer: "440 works."
- **The bytes:** checked by `claude-debug/native-n5/g-wav.py` on dumps from `PLUMMER_DUMP_WAV`. For both graphs:
  - the header is right: PCM, 1 channel, the graph's rate, byte rate, align, 16 bits, and the data length;
  - every sample equals round(lent × 32767), so the WAVE is exactly the samples the core lent through `media.get`;
  - the spectrum is what the graph makes. Graph 1 has harmonics of 80 Hz (79.4, 161, 241, 319 and 400 Hz). Graph 2 is 440 Hz with odd harmonics at 1320 Hz (−19 dB) and 2200 Hz (−28 dB), a triangle's 1/n².
- **No playback bug.**
- **The conversion is pinned by a test, not yet run:** it now lives in `PlumCraft.Kit.Sound.Wave`, and `SoundTests` covers it: a sine at 48, 44.1 and 24 kHz, mono and stereo, checking the header, the amplitude and a 440 Hz peak, plus clipping and NaN. The VM went down during `dotnet test`, so the test has not run.
- **Autoplay (B92):** in two of six double-click openings, the Play button's Click handler ran with no click or key of the script's on it. Three later tries, with every pointer and key on Play logged, did not reproduce it. Cause not found. The Apple apps and egui were not checked.
- **New bug, B93:** a graph opened from the command line is titled Untitled.

### Merged

The dev branch at `e36ee7b` is merged: PR #155, with `viewer.grid`, the pick-list form field, 57 cases, and B80 to B88.

Not yet adapted on Windows:

- the grid's column buttons through `viewer.grid`;
- the pick-list form field.

The C# runner's new matchers wait on `origin/fix/manifest-warn-2`; I did not check whether it has merged.

## Third session (2026-11-22, the VM's TPM device off)

### Timeline

| Time (Mac) | Step | Available MB | Ping |
|---|---|---|---|
| 09:28 | dev branch merged (`deda565`, fast-forward; my BUGS entries re-applied, keeping the dev branch's fixed B56, B76, B86 and B87) | n/a | watch from 09:28:57 |
| 09:29 | VM checkout reset to `deda565` and synced; 648 files' hashes the same on both | 7200 | no loss |
| 09:29 to 09:30 | core DLL and engine programs (cargo, 63 s), then the app (17 s) | 6959 after | no loss |
| 09:30:52 to 09:31:30 | `dotnet test`: 48 of 51 (only the expected-skip lists failed: the new install case) | 6959 before | no loss |
| 09:32:39 to 09:33:21 | `dotnet test` again: 51 of 52 (only the new copied-manifest test failed: no `platform.subprocess` in its core) | 7108 before | no loss |
| 09:33 to 09:34 | that test alone, three times; passed after the fix | n/a | no loss |
| 09:34:29 | person check started (`person-check.ps1`, the app driven by SendInput and UI Automation) | 7187 before | no loss |
| 09:35:29 | **ping lost**, and lost at every try since; SSH timed out at 09:36:17 | n/a | lost |

**How long the VM stayed up:** with its TPM off, it was up from its 09:11 boot through the controller's 15-minute idle watch, then cargo, the app build and four `dotnet test` runs. It froze about a minute into the person check, with one Plummer under UI Automation and input. The TPM is therefore not the cause, or not the only cause.

The three freezes since yesterday each came during or within about two minutes of a script driving the app through UI Automation and SendInput:

1. Yesterday evening: a minute after the person check's second run ended.
2. Last night: a minute after the last double-click tries, just as `dotnet test` started.
3. Today: during the person check.

Many other such runs did not freeze it: the sound set-ups, and the double-click tries before the last ones. This is a pattern to look at, not a cause.

### Changed this session

- **The C# runner** has `$includes`, and `$deep` fields whose wants may be matchers (a missing field is matched too). The kit's driver can claim more capabilities (`Also`). `CopiedManifest.Make` builds the suite's copied-manifest engines folder on Windows (`uv.cmd` fails as Python would). A new kit test, `TheCopiedManifestStepsPassThroughTheKit`, runs the manifests case's copied-manifest steps with none skipped. It passed on the VM.
- **The kit's conformance tests** expect the install pick-list case to be skipped by cores that can't install, as the Rust driver does.
- **`dotnet test`** on the VM ran 52 tests, and every one passed across the runs above: `SoundTests` and B52's waiting test among them, in three full runs. The last full run had only the copied-manifest test failing, before its fix; that test passed alone afterwards.

### Not done (the VM froze)

- the person check end to end;
- B91 by a connected separate-process run;
- B72 on Windows;
- the wire tiles measured and compared;
- the banner looked at;
- the retakes;
- `--bench-panels`;
- the conformance runs;
- the DuckDB install;
- on Windows: `viewer.grid` column stepping, the pick-list ComboBox, the item and engine `warning`;
- the CLI claiming `copied_manifest` against the app.

## Fourth session (2026-11-22: TPM off, display without GPU acceleration)

The VM was set up by the maintainer: TPM off, display "Red Hat VirtIO GPU DOD controller" (no 3D), console signed in at 09:52. A ping watcher ran from 09:54:07 and lost no ping. Its 30 losses are all from the 09:35 freeze.

### Timeline

| Time (Mac) | Step | Available MB |
|---|---|---|
| 09:53 | VM checked: VirtIO GPU DOD, 1512 × 909; a stale `plummer-person-check` task from the 09:35 freeze removed | 7013 |
| 09:54:29 to 09:56:40 | person check, run 1: survived; 11 PASS, 5 FAIL (the palette, B94) | 6986 to 6901 |
| 09:57:08 to 09:59:16 | person check, run 2: survived; 11 PASS, 5 FAIL (the same) | 6921 to 7093 |
| 09:59:47 | app rebuilt with B94 fixed | n/a |
| 10:00:11 to 10:01:13 | person check, run 3: survived; 15 PASS, 1 FAIL (the check's own reading of a widget's value, fixed after) | 6962 to 6966 |
| 10:02 to 10:03 | warning glyph, pick list, `viewer.grid` built; `dotnet test` 52 of 52 | n/a |
| 10:03:58 to 10:07:36 | conformance, ARM64, three runs: 57 of 57 each, 767 of 772 steps (5 copied-manifest steps not claimed) | 6967 to 6988 |
| 10:07:41 to 10:10:33 | x64 core and app built, conformance on x64: 57 of 57 | 6846 |
| 10:10:53 | read half with the copied engines folder and `--has copied_manifest`: 36 of 36, 461 of 461 steps, none skipped | n/a |
| 10:11:53 to 10:12:44 | B72 and the banner, chosen by clicks: screenshots | n/a |
| 10:12:58 to 10:14:33 | canvas bench, tiles and layers; pixel comparison at zoom 1 | n/a |
| 10:15:06 to 10:16:57 | retakes: edit-number, edit-choice, context, light and dark | n/a |
| 10:17:11 to 10:18:28 | `--bench-panels`, three runs | n/a |
| 10:19:58 to 10:21:29 | DuckDB installed with uv into scratch folders, used, a node run | 7000 |
| 10:21 to 10:22 | pixel comparison at zoom 0.37 | 6900 |

**How long the VM stayed up:** from 09:52 to past 10:23 under the whole load above, one app at a time, with no lost ping. That is three person checks, every build, five conformance runs, the benches and an install. The display without GPU acceleration is the only change from the 09:35 freeze.

### Results

- **The person check, run 3:** 15 of 16 passed.
  - Passed: the window, New Graph and closing it, the Engine menu, Settings, Text as a separate process (connected), Attach…'s form, Install Engine…'s form, a palette node given a value, a second wired to it, Run to "HELLO PERSON", the pop-out showing it, Save As, close, reopen with both nodes, Quit.
  - Failed: the check's reading of the typed value. The run's result shows the value was set.
- **B91:** verified (the separate-process engine connected and ran).
- **B94** (the palette's rows unnamed for UI Automation): found and fixed.
- **B72 on Windows:** choosing the copied engine by clicks failed within 3 s. The floating banner showed its stderr ("No module named copied_engine") and the copy hint; the screenshot was looked at.
- **The floating banner:** looked at in that screenshot. It sits over the canvas's top edge on an opaque card with a shadow.
- **The warning glyph:** the Engine menu shows ⚠ before Copied Engine (screenshot looked at). No screenshot caught the tooltip. The engine cell's warning was not seen: no engine in use had a warning.
- **Conformance:** 57 of 57 on ARM64 three times and on x64 once (`--has manifests --has install`). The copied-manifest steps also pass against the app: 461 of 461 in the read half.
- **`dotnet test`:** 52 of 52. This includes the new copied-manifest kit test, `SoundTests` and B52's test.
- **Wire tiles**, measured under software rendering, so not gate evidence for smoothness. 500 nodes, 800 wires, one run each:

  | Phase | Tiles: fps / late frames | Layers: fps / late frames |
  |---|---|---|
  | pan | 58 / 6 | 55 / 15 |
  | pan at fit | 59 / 0 | 52.8 / 29 |
  | zoom | 58 / 1 | 55.2 / 12 |
  | face drag | 58.5 / 3 | 57.8 / 14 |
  | wire drag | 58.2 / 1 | 58.2 / 3 |

  Tiles: 98 tiles, 28 MB of pictures.
- **Pixel comparison of the same view:**
  - At zoom 1, the largest difference is 5 of 255, in 1.1% of pixels (anti-aliasing).
  - At zoom 0.37, 2.7% of pixels differ by more than 32, all inside the dense bands of wires: the moiré of hundreds of thin crossing lines differs between a raster sampled at 0.375 and vector strokes at 0.37.
  - Not judged a defect, but a person should look: `claude-debug/native-n5/shots-r4/wiresz-*.png`.
- **`--bench-panels`**, three runs, software rendering:
  - opening each panel and viewer: 35 to 835 ms (pictures, SVG and the table slowest);
  - the table's 100,000 rows (500 a page) scrolled at 52.5 to 58 fps, with 0 to 28 late frames in a 4 s phase;
  - a page turned through the core: 87 to 154 ms (p50).
- **DuckDB install:** done through the core's form ask (`engine.install`, the repository answered from the pick list), using uv on PATH. Every uv folder was redirected to scratch: tools, bin, cache and Python, plus the engines folder.
  - Timing: uv downloaded and built it (numpy among its packages) and registered `duckdb-engine` in 35 s.
  - `engine.use duckdb` then worked, and its kinds appeared (`duckdb.read`, `duckdb.query`, …).
  - A `duckdb.query` node with `select 6 * 7` ran: the second identical node showed "cached · 34 ms". The value itself was not read back.
  - No failures on the way.
- **Retakes:** edit-number, edit-choice and context, light and dark. Looked at: the number field over its row, the choice drop-down, the node's context menu. The other slice 4 scenes were not retaken.
- **Merged-contract changes on Windows:**
  - the grid's column window through `viewer.grid {cols}`, with buttons from `prevCols`/`nextCols`;
  - the pick-list field as a ComboBox under its label, editable when `other` is set;
  - the menu item `warning` glyph and tooltip;
  - `engine.warning` in the engine cell;
  - the C# runner's `$includes`, matcher-valued `$deep` and `copied_manifest`.

  The suite's grid case ("a wide array's numbers are shown 64 columns at a time") and the install pick-list case passed against the app, through the core. The grid buttons and the ComboBox themselves were not looked at.
- **The Swift runner against the Windows app** (rebuilt once the Mac's disk had 106 GB free, 10:24): read half 36 of 36 (456 of 461 steps, the copied-manifest steps not claimed), edit half 21 of 21.
- **Not done:**
  - the other slice 4 retakes.

## Fifth session (2026-11-22, branch `native/n5-gate-2`, from the dev branch at `8a983c2`)

The VM had no TPM and the VirtIO GPU DOD display. A ping watcher ran from 10:31; no ping was lost.

### Timeline

| Time (Mac) | Step | Available MB |
|---|---|---|
| 10:31 | VM checkout reset to `8a983c2` and synced (648 files the same); app built | 7010 |
| 10:32:09 to 10:32:56 | `dotnet test`: 53 of 53, with the B93 test | n/a |
| 10:33:05 to 10:36:33 | person check three times: 17 of 17 each | 6977 to 6813 |
| 10:36 to 10:39 | grid buttons, pick list, the warned engine in use and its tooltip, the one-line status bar: screenshots | n/a |
| 10:39:59 to 10:46:05 | the slice 4 scenes retaken (light) | n/a |
| 10:46 to 10:47 | floor checks in the launched app (`g-floor-checks.ps1`) | n/a |
| 10:47:53 to 10:50:34 | C# runner ARM64 57 of 57; x64 built, 57 of 57; Swift runner 57 of 57 | n/a |

### What was done and seen

- **B93, fixed:**
  - Cause: Windows only. The core announces a document's state when it opens it, and the command line's graph is opened before its window's model follows the feed, so the announcement was missed. The core's state was right. The Mac titles its windows from NSDocument's file and egui from the core's title, so neither had the bug.
  - Fix: the model reads the document's state once from `ui.inspect` at start, as it did the engine's.
  - Test: `AGraphOpenedBeforeItsModelIsKnownByItsFile`.
  - Person-check step 14: the saved graph opened from the command line is titled "person check.json - Plummer", with both its nodes. Passed three times.
  - No core change.
- **The person check:** 17 of 17, three runs in a row on the final build.
- **The numbers grid's window, looked at:**
  - first window: caption "columns 0–63 of 150", ‹ disabled, › enabled;
  - middle window: "64–127 of 150", both enabled;
  - last window: "128–149 of 150", › disabled.
  - Every step went through `viewer.grid {cols}`.
- **Install Engine…'s pick list, looked at:**
  - an editable ComboBox under its label "Repository", with the hint "owner/repo or https://github.com/owner/repo";
  - dropped down by a click on its chevron, it lists DuckDB, OpenCV and Atlas.
  - Without `other`: no form in the product has one (the install form is the only pick list), so it was not seen. The code draws it non-editable.
- **The engine cell's warning, looked at:** a warned manifest that starts was used (`g-copied-ok.ps1`: the copied manifest's stand-in `uv` starts the text engine's own program). With that engine in use the status line read "engine: Text + Text 0.7.0 · 2 engines · connected · 47 nodes ⚠". Hovering the ⚠ showed the whole warning.
  - The warning names the folder with Windows' `\\?\` prefix: **B95**, the core's, recorded.
- **The status bar's message:** now one line, cut with an ellipsis, with the whole text on hover. Seen with the copied engine's error.
- **Text's hosting in the Engine menu:** by design. The engine in use adds its mode to its heading (`menu::engine_items`: `"{label} · {mode_label}"` when it is the engine in use and live), so Text shows "Rust · in-process (mode 1)" and Array, not in use, shows its name. The Mac's menu draws the same label from the same model (from reading; the Mac app was not launched).
- **Slice 4 scenes retaken in light:** compare, json (tree, YAML), marquee, media (audio, picture, svg, array, grid), params, picture, recipe, reports (messages, cache, graph parameters), settings (three pages), table, text and scalar; looked at as a contact sheet.
  - Not retaken: the canvas tags and the value editor. Their steps files no longer suit the core (`net.connect` refused without a wire drawn through a net label; text.literal names no editor). The steps need rewriting, not the app.
- **Floor checks in the launched app:**
  - Run Selected (F27);
  - latest wins (F29);
  - auto-run (F30);
  - a failure standing in for the reply (F35);
  - cancel (F36), with the Slow node "running 24%" then "cancelled";
  - plus the recipe scene (F56).
- **The floor now:** 44 met, 3 partly, 5 not met, 34 claimed on reading alone. The rest of the reading rows have no case or check that launches the app on their path: core-only behaviour (F5, F40, F69, F84), the research software's (F65, F66), or kinds the text and array engines don't have (F14, F15, F17 to F20, F43 to F45, F55, F61 to F63).
- **Gate:**
  - `dotnet test`: 53 of 53;
  - C# runner: ARM64 57 of 57, x64 57 of 57;
  - Swift runner against the app: 57 of 57;
  - person check: 17 of 17 three times;
  - private strings: clean.

  All on the display without GPU acceleration, so no timing here is smoothness evidence.

## Sixth session (2026-11-22, branch `native/n5-gate-3`, from the dev branch at `fe89cfc`)

### B95, in the core

The helper is `plumcraft_proto::programs::shown(&Path)` and `shown_str(&str)`. It is plain string handling:

- `\\?\C:\x` becomes `C:\x`;
- `\\?\UNC\server\share` becomes `\\server\share`;
- anything else is unchanged, including `\\?\GLOBALROOT\…`, which has no other spelling.

It is used everywhere a canonicalised path reaches words a person reads:

- `crates/host/src/manifest.rs`: the four copied-manifest warnings (project folder missing or without `pyproject.toml`; no-project base folder missing or without a project).
- `crates/editor-core/src/engines.rs`, in `load_manifests`: the one `problems.push` for an id already loaded (`x.resolved.path`). This is one line before `cargo fmt`, now lines 324 to 326. It is the other agent's file; no other line was touched.
- `crates/engine/src/register.rs`: `register`'s uv-cache warning and refusal (three `root` displays).

Places checked and left alone, because their paths are used for file operations or a protocol, not shown to a person: the manifest's real folder, command and cwd; the craft engine's `--craft` argument; the CAS folders; the store's trusted paths; the manifest that `register` writes. No `\\?\` reaches a spawn error: those messages don't name the program.

Tests:

- `a_path_shown_to_a_person_has_no_long_path_prefix` runs everywhere;
- `a_canonical_windows_path_is_shown_without_its_prefix` runs on Windows only, through `canonicalize`;
- the copied-manifest test now asserts that the warning names the folder as `shown` gives it and contains no `\\?\`.

Gate:

- **Mac:** `cargo fmt --check`, clippy (`-D warnings`) and nextest of proto, host, engine and editor-core: 377 passed, 2 skipped.
- **Windows test VM:** both new tests pass.
  - Six other tests in those crates fail on Windows with or without the change, so they were already failing: **B97**. I checked by stashing the change on the VM and running them again.
  - The host test reaches the new assertion before its old failure.
- **Seen in the app:** the warned engine's tooltip now reads `uv run --project . names C:\Users\…\copied-ok\engines, with no pyproject.toml…` (`shots-r4/warned-tooltip-b95.png`).

### Scenes

- **Canvas tags:** the steps were rewritten. The graph is made in the steps; a wire is labelled as the net "words", then Lower case is connected to it by name. Retaken in light and dark and looked at: "words ×2" at the driver, "words" at each use.
- **Reports:** the steps were rewritten without the value editor and retaken in light and dark: Messages, Cache Report, graph Parameters.
- **The value editor cannot be shown.** No node of the text or array engines names an editor (`node.editor` refuses with "text.literal names no editor"), and the VM has no engine that does. F22 stays partly.

### Windows gate

| Time (Mac) | Step | Result |
|---|---|---|
| 11:08 to 11:12 | sync to `fe89cfc` plus the change; DLL and engine programs; the touched crates' tests | as above |
| 11:12:55 | app built; `dotnet test` | 53 of 53 |
| 11:13:51 to 11:17:14 | person check, three runs | 17 of 17 each |
| 11:19 to 11:23:50 | C# runner on ARM64 and x64, and the Swift runner | 57 of 57 each |
| 11:24 | warned engine hover picture | the path is shown clean |
| 11:24:30 to 11:26:01 | canvas and reports scenes, light and dark | retaken |

The x64 run had to be repeated. The first x64 core build failed to compile: the sync keeps the Mac's file times, older than the VM's last x64 build, so cargo had not rebuilt `plumcraft-proto`. That x64 conformance run therefore used the old core. After touching the file, the x64 core and app were rebuilt and the suite run again: 57 of 57.

No ping was lost; available memory stayed between 6.8 and 6.9 GB.

## Seventh session (2026-11-22, branch `native/n5-save`, on the dev branch at `709a2a7`)

The VM had no TPM and the display without GPU acceleration. It was restarted at midday, after the Mac hibernated on a flat battery. The ping watcher ran from 15:20:50; no ping was lost.

### Save Result on Windows

- **The footer tap is the core's (B122, merged as PR #166).** The Windows canvas now reports each footer line as `parts.footer[k]` (`FaceLayout.FooterParts`, `CanvasLink.ReportPartsAsync`). I removed the shell-side click I had written before the core's change, and the `ask: true` special case in `DocumentModel.ActivateAsync`. The C# runner claims `footer`.
- **The rest needed no new drawing.** The core's `file.save` ask shows as Windows' Save As dialog, with the output's formats as file types ("Quoted Text", then "Text"). The options form shows as a dialog: "Save as Quoted Text", with its Quote mark as a pick list. Both are pictured in `g5-pc-shots/save-picker.png` and `save-options.png`, and I looked at them.
- **The C# runner** gives each case a `$scratch` folder (`--scratch`) and claims `saves`. The test engine (`plumcraft-ports-engine.exe`, built with the core) is installed in the app's engines folder for the launched runs (`g-ports.ps1`), with `--engine ports --has ports_installed`. The audio engine runs in process (`--engine audio`).
- **Person check, step 15.** In the graph reopened from the command line:
  - turn Pick Engines Automatically back on;
  - choose Engine › Ports (test engine);
  - add Edge and Result from the palette, wire them, Run;
  - click Result's Save Result… line;
  - in the Save As dialog, type the path;
  - accept the options form's default quote mark;
  - read the file back: `"[edge]"`.

### B98, in the core

Save Result's path went through the rule for a path a workflow stores: relative to the workflow's or start folder, with `/` between folders. On Windows the person read `C:\…\conform-scratch/case-18/plain.txt` (inside the start folder) or `C:/Users/…` (outside it).

The fix, at the one place in `invoke.rs`: `save_choice` keeps the path as chosen; only macOS's `:` becomes folders. It has a unit test with Windows-style paths inside and outside a folder.

Other chosen paths a person reads back, checked by reading:

- Save As of a workflow and Export Bundle pass the chosen path as it is;
- a viewer's Save As goes through Save Result, so it is fixed with it;
- Drag Out writes a file the core names;
- path inputs keep the stored form on purpose.

The case passed against the app with its scratch folder inside the start folder.

### B97, fixed at source

All six now pass on the Windows test VM. On the Mac, fmt, clippy (`-D warnings`) and nextest of editor-core, host, engine and proto pass: 400, after merging the dev branch.

| Test | Fix |
|---|---|
| host: the two absolute-path cases | the rule treats a path with a root but no drive (`\elsewhere`) as not the manifest's folder (`from_here`) |
| engine: register from uv's cache | reads the written manifest as TOML |
| editor-core: a desktop offers every hosting | expects a process hosting disabled exactly when this build hasn't made its program |
| editor-core: switching without a window | adds the process hosting as a choice before refusing it |
| editor-core: the exit-at-once test | compares the printed folder with `programs::shown` |

### Also

- **PR #167:** the pop-out's Recipe shows only when `hasRecipe`. Save As's tooltip promises no recipe. Neither was looked at.
- **The Python SDK's `register` on Windows:** given `--folder \\?\C:\…\eng`, it printed `registered weather: C:\…\eng\weather.toml` (`g5-pyreg.ps1`).
- **B124 probe:** my Mac nextest run of editor-core, host and engine, with a scratch home, wrote no preferences under it.
- **B99:** one edit-half run (the second of four on ARM64) failed "a kind chosen from a dropped wire's palette…" at step 14, with no `port` on the palette's item. It was not seen again in the next six runs on ARM64 and x64.

### Results

| Check | Result |
|---|---|
| `dotnet test` | 53 of 53, three times this session |
| C# runner against the app, test engine installed | ARM64 64 of 64 (936 of 941 steps, the five copied-manifest steps not claimed), three runs on the final build; x64 64 of 64, two runs |
| Swift runner against the app from the Mac | 60 of 60, the four ports cases skipped (no test engine in that launch, and its `$scratch` would be on the Mac) |
| Person check on the final build | 18 of 18, three runs in a row (16:22:01, 16:24:06, 16:26:01 Mac clock) |
| Floor | 53 met, 4 partly, 5 not met, 24 claimed on reading alone. Newly met: F14, F15, F17, F18, F20, F21, F43, F45, F72; F16 and F64 partly |

## Eighth session (2026-11-22, branch `native/n5-prompts`, on the dev branch at `798e8bb`)

The VM went off the network at 19:13, before any step of this session had touched it. Its window stayed up; UTM then died, its emulator process hung while exiting and blocked a restart, and the maintainer rebooted the Mac. This is not B99. Work resumed at 20:34; from then the ping watcher lost no ping.

### What changed

- **Prompts in their own words.** A prompt's buttons show its `labels`, falling back to Windows' words. Unsaved changes keeps "Don't Save". The save's question shows "Save as PNG anyway", "Save as NumPy (.npy)" and "Cancel", and its detail reads "As NumPy, it would be saved as strip.npy." instead of JSON. Pictured in `g6-pc-shots/save-shape-final.png`, and looked at.
- **Person check step 16, `recipe`.** Traced's pop-out offers Recipe; its flyout reads `{"params": {"seed": "plum"}, "style": "traced"}`. Result's pop-out has no Recipe button. Pictured in `recipe-traced.png` and `recipe-none.png`, and looked at. This is also the first look on screen at last session's Recipe change.
- **Person check step 17, `save-shape`.** Strip's Save Result… with a `.png` name brings up the core's question with the three labelled buttons. "Save as NumPy (.npy)" writes `strip.npy` (16128 bytes, starting with the NumPy magic). A second pass with "Save as PNG anyway" writes a PNG (717 bytes).
- **Person check step 13, `quit`.** One run in five left the app running after File › Quit, with a popup showing; it had not been seen before. The step now:
  - clicks Quit where the menu settles;
  - says what it found;
  - clicks again if the menu is still open;
  - takes a screenshot if it fails.

  On the three gate runs, Quit was clicked once each time and the app ended. The cause of the one failure is not known.

### Durations, expected and actual

| Step | Expected | Actual |
|---|---|---|
| ARM64 core and engines build | about 8 min | 80 s |
| ARM64 app build | about 3 min | 21 s |
| Person check, first trial of steps 16 and 17 | about 3 min | about 4.5 min (two steps failed: nodes placed off screen, the wrong pop-out window) |
| Person check, second trial | about 3.5 min | 148 s, 20 of 20 |
| C# runner, ARM64, test engine installed | about 5 min | 90 s (app rebuild included) |
| x64 core, app and C# runner | about 5 min | 197 s |
| `dotnet test` | about 2 min | 50 s, plus 50 s for a second run I started by mistake |
| Swift runner from the Mac | about 4 min | 40 s, plus 25 s to rebuild it (Messages.swift had changed) |
| Person check three times, first try | about 8 min | 373 s; run 3 failed at Quit |
| Person check three times, final | about 7.5 min | 442 s |

### The gate

| Check | Result |
|---|---|
| `dotnet test` | 53 of 53 |
| C# runner, test engine installed | ARM64 66 of 66 (37 read, 29 edit); x64 66 of 66 |
| Swift runner from the Mac | 60 of 60; the six ports cases skipped (not claimed) |
| Person check | 20 of 20, three runs in a row (21:00:02, 21:02:33, 21:04:56 Mac clock) |
| Floor | 54 met, 4 partly, 5 not met, 23 claimed on reading alone. F56 is now met by the person check's launch, not a scene; F64 stays partly (no picture saved with its recipe) |

### Noticed, not changed

- The question's text begins with a lowercase letter ("this 4 by 1000 array is…"). The words are the engine SDK's.
- The Save As dialog's "Save as type" box was not read by step 17 (its name didn't match), so the report's "picker type" is empty. It is not part of the step's pass.

## Ninth session (2026-11-23, branch `native/n5-lists`, on the dev branch at `ede5705`)

The VM has had no internet since the Mac's reboot; the controller can't restore it and the maintainer is away. So:

- The new commits went to the VM as a git bundle.
- NuGet's vulnerability audit, which can't reach nuget.org (NU1900, an error in this build), is off per build in the scratch scripts (`-p:NuGetAudit=false`). Nothing on the VM changed.
- See "A slow first start" below for the third effect.

### A list value as named pictures (contract 8.7)

- **The scene's preview** has `caption` (`Preview.Caption`).
- **A list's face picture** has a tag on its bottom-left corner: "caption · n pictures", or "n items" with no caption. It is white on a dark band, as the Apple canvas draws it (`ListNames.Tag`). UI Automation sees it as the face's picture, named by the tag.
- **The pop-out's stepper** names each item by its sheet label ("text a · 1 of 4"), with a pick list of all the names (`ListNames.Names`, `ListNames.Title`). With no names it says "item 5 of 11" as before.
  - It sits on a line of its own under the bar. Beside View As and the bar's buttons, a 560-pixel pop-out cut the name off; the pictures showed that, and the fix was looked at.
- **Unit tests** (`ListTests.cs`): the fixture's list picture is tagged "first · 3 pictures", plus the edge cases of the tag and the names.
- **Person check step 18, `list-names`.** Items' four words are wired into Result, which then runs once a word. Its pop-out's stepper reads "text a · 1 of 4"; its pick list offers text a to text d; choosing text c moves it to "text c · 3 of 4". Pictured and looked at.
- **Unseen on Windows: the face tag with a caption.** Neither SDK puts a caption or an item count on a node's thumbnail (`exec::thumb` passes an empty caption; `plane.rgba8` sets `items: 0`). So no public engine on Windows makes a list face picture: only the research software's engine does. The tag is tested on the fixture alone.

### Core changes that reached Windows, launched

| Change | On Windows |
|---|---|
| The save question begins with a capital (B127) | Seen: step 17 reads "This 4 by 1000 array is 4 pixels on its shorter side…" and checks the capital |
| A pack's type by its short name, the full id on hover | The name was seen: Tuning's output reads "tuning · tune". The hover was missing: the Windows canvas showed no tooltip on a pin. Added: a pin's tooltip is its row's `hover`. Step 19 sees "Type: x.ports.tune" in it, pictured |
| A read-only value with no summary is summed up | Unseen: it needs a port with a widget the core has no editor for, and no engine on Windows has one. The Windows face draws the core's `text` for such a row as it is |
| A focus loss commits only what was typed (B129) | Windows had the same fault: its row field's `LostFocus` committed whatever the field held. Fixed at source: `RowEditor.FocusLostAsync` commits only text that differs from what the field opened with; otherwise it sets nothing and stays open. Test: `AFocusLossCommitsOnlyWhatWasTyped` (a script's `row.set` while the field is open survives a focus loss) |

### A slow first start, and why

After the builds, the app took 49 s to start; the core's open alone took 24 s. Time-boxed to 20 minutes on the VM:

- **`plum_open` by itself took 7 to 9 ms offline in every configuration.** The probe `g7-opentime.ps1` tried: bare, the app's engines and platform, a control port, the engines folder, and the app's full config.
- **Loading the core's DLL took 8.3 s the first time, 2 ms after.**
- **The same builds started quickly a few minutes later.** The ARM64 app was ready in 0.9 s, with the core's open at 43 ms. The freshly built x64 app's first launch was not ready in 90 s; its second was ready in 25.6 s, with the core at 349 ms.
- **Defender on the VM:** real-time protection is on, and cloud protection is at its advanced level (MAPS 2) with sample submission. It logs a 5007 configuration event about every 20 s.

Reading: Defender holds newly built files at their first use while it waits for a cloud verdict it can't get offline. The core is not the cause, so there is no bug entry. Each rebuild is now followed by one warm-up launch before the checks, and the timeline records it.

The person check gains `-SlowStartWaitS N`, off by default, for a machine known to start slowly; the report says when it was used. The gate runs below did not use it.

### Floor

F61 (list mapping with axes) moves to partly: step 18 maps a node over one list and steps through it by name. Two lists crossed have not been launched. F55 (sheet) stays claimed on reading: the stepper reads the sheet's labels, but the sheet view was not opened.

### The gate, against the last baseline

| Check | Baseline | Now |
|---|---|---|
| `dotnet test` | 53 of 53 | 58 of 58 (4 list tests and the B129 test added) |
| C# runner, ARM64, test engine installed | 66 of 66 | 66 of 66 on the final build. One run before it was 65 of 66: "the engine's run changed while it goes" failed at step 26, `superseded` unchanged, because the first run had finished before the second `run.start` (a timing race in the case) |
| C# runner, x64, test engine installed | 66 of 66 | 66 of 66 (before the stepper moved to its own line, a pop-out layout change only) |
| Swift runner from the Mac | 60 of 60 | 60 of 60; the six ports cases skipped (not claimed) |
| Person check | 20 of 20, three runs | 22 of 22, three runs in a row with the default wait (Mac 10:44:51, 10:47:41, 10:50:27) |
| Floor | 53 met, 4 partly, 5 not met, 24 reading | 54 met, 4 partly, 5 not met, 23 reading: F61 to partly |

### Durations, expected and actual

| Step | Expected | Actual |
|---|---|---|
| ARM64 core build | 3 min | 46 s |
| `dotnet test`, first run | 1 min | 205 s (Defender's hold on the new test binaries); 42 s on the final build |
| The open-time investigation | 20 min (time-box) | about 5 min on the VM |
| C# runner ARM64 | 4 min | 212 s, then 178 s, then 68 s |
| x64 build and runner | 6 min | 163 s with the runner unable to start (the first launch held), then 131 s |
| Person check trials (4) | 3.5 to 4 min each | 112 s (failed at the start), 243 s, 215 s, 176 s |
| Final build, warm-up, `dotnet test`, both runners | 9 min | 228 s |
| Person check three times | 10 min | 502 s |

## Tenth session (2026-11-23, branch `fix/b133-windows-browse`, on the dev branch at `d29630c`)

B133's Windows half: Browse for a path row. The VM is still off the internet: the commits go over by git bundle, and each rebuild is followed by one warm-up launch (Defender holds new files; see the ninth session).

### What was built

- **The button.** A row whose widget has `pathMode` draws the path's text, a rule, and an accent "…" in the core's square (`PathBrowse.Square`, the same rule as `geometry::browse_rect`).
  - UI Automation sees it as a "Browse" button.
  - The tap is the core's gesture (`selection.set`, `row.browse`); the shell adds no tap handling.
  - The field editor stops short of the square.
- **The dialog.** `Asks.cs` already answered `file.open`, `file.save` and `folder.pick` with the WinRT pickers. Those can only start in a known library, and they ignored the core's `startIn` and the current value.
  - For purpose `path` (and `files`), the system's common item dialog is now used, through COM (`PathDialogs`, no unsafe code).
  - It opens in the current value's folder, else in `startIn`.
  - Its filters are the hint's kinds, then All files.
  - It fills in the file's name and gives back real paths.
  - Every other ask keeps its picker.
- **Browse with the field open.** A press on Browse takes focus from the field, and B141's rule would then commit what was typed. A focus loss with the pointer over the open row's Browse now commits nothing, so the core's `row.browse` closes the field without its text, as the contract says.
- **Unit tests** (`PathBrowseTests`): which widgets offer Browse, the square with the core test's own numbers, the field short of it, the filters, and the start folder.
- **Person check step 20, `path-browse`:**
  - Read Text File's Browse opens "Choose a File" in the value's folder (`…\pc\browse`), with "Text (*.txt)" as its filter and the file's name filled in. It is read through UI Automation, pictured (`g8-pc-shots/browse-dialog.png`) and cancelled; Open is never pressed.
  - Then text is typed into the open field (pictured, `browse-typing.png`) and Browse pressed: the dialog comes, it is cancelled, and the value is unchanged.
- **The scripted answer** is the conformance case "ports: a path input's Browse asks for a file by its hints, and the answer sets the value" (one undo step). It passes on the C# runner on ARM64 and x64.

### Also in this branch

- **`InRow.Editor`** (decode only). #181 added `editor` to input rows, and the Windows model's strict decode tests failed on the merged dev branch (57 of 62). FR1's drawing and tap on Windows follow on `native/n5-editor-row`.
- **The person check's `Say`** retries a write that meets the report being read (`g-tail.sh`). One trial stopped on it.

### Durations, expected and actual

| Step | Expected | Actual |
|---|---|---|
| ARM64 core, engine, app, warm-up, `dotnet test` (first) | 6 min | 392 s (warm-up 130 s: four holds of about 24 s on new files) |
| Person check trials (4) | 4.5 min each | 159 s (stopped by the report-file collision), 202 s, 199 s, then 252 s with a rebuild |
| Gate on `7fad6dc`: C# runner ARM64 and x64, Swift runner | 9 min | 522 s |
| The merge, then gate part 1 on `d29630c` | 10 min | 665 s |
| `dotnet test` after the decode fix | 3 min | 177 s |
| App rebuilds and warm-ups, C# runner ARM64 and x64, person check x3 | 20 min | 1,164 s |

### The gate on the merged dev branch

| Check | Baseline | Now |
|---|---|---|
| `dotnet test` | 58 of 58 | 62 of 62 (`PathBrowseTests`, four) |
| C# runner, ARM64, test engine installed | 66 of 66 | 68 of 68 (37 read, 31 edit: the Browse case and #181's case added) |
| C# runner, x64, test engine installed | 66 of 66 | 68 of 68 |
| Swift runner from the Mac | 60 of 60 | 60 of 60, 8 skipped: the ports cases, now including Browse (run before the decode-only model change) |
| Person check | 22 of 22 | 23 of 23, three runs in a row (Mac 12:32:52, 12:36:20, 12:39:27) |
| Floor | 53 met, 5 partly, 5 not met, 23 reading | 54 met, 4 partly, 5 not met, 23 reading: F16 met |

On `7fad6dc`, before the merge, the ARM64 runs hit B99's case twice: the C# runner (66 of 67) and the Swift runner (59 of 60). The sighting is recorded in B99. The x64 run that round passed it, and every run on `d29630c` passed it.

### B133 on Windows

It can be closed for Windows. What remains of B133 is the iPad's Photos source, which is another agent's.
