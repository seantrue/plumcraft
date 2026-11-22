# Bugs

Bugs reported in passing (`bug: …`), kept until fixed. Numbers are never reused. Entries with an issue are also tracked on GitHub.

## Open

### B85. Plummer crashed twice in Swift's main-actor check from an AppKit callback while a script step drove a panel from inside a task

- Reported: 2026-11-21
- Report: found by the person check's first runs (branch `native/n4-person-gate`)
- Context: two crash reports (`Plummer-2026-11-21-210954.ips`, `…-211213.ips`, SIGSEGV): `swift_task_isCurrentExecutor` read a bad executor in `@objc CanvasView.isFlipped.getter` (during a menu's tracking) and in `@objc GraphWindowController.validateToolbarItem(_:)` (a toolbar validation timer), each right after the check's step had called the save panel's `ok(_:)` from inside the inbox's main-actor task. The likely cause: AppKit work that runs the run loop (a panel, a menu's tracking) inside a task's job lets other main-actor code run with that task's executor still current. A person's pick never runs inside a task, so this is probably the check's own defect; not shown to be impossible for the app (the `--then` steps' `menu:` and `context:` holds also run inside a task). Since the person check's steps run from the run loop itself (`PersonSteps.onRunLoop`), ten runs have not crashed.
- Issue: none

### B88. The Mac conformance case "the more-button opens the node's menu" failed once: the tap hit nothing

- Reported: 2026-11-21
- Report: the person gate branch's baseline run of `native/apple/PlumCraftMac/conformance.sh all` on the dev branch as it was (`d59d5ba`), before any change
- Context: step 3, `ui.gesture {kind: "tap", at: [284.126, 14.0]}` on the more-button's part the shell had measured: the core answered `hit: none` and cleared the selection, so no node menu came back (54 of 55). The next three Mac runs that day passed every case. Possibly the tap arriving before the app reported the header's parts (`geometry.parts`); not looked into.
- Issue: none

### B96. The wire tiles' moiré at low zoom differs slightly from the per-wire drawing (observation, low priority)

- Reported: 2026-11-22
- Report: the N5 gate's pixel comparison (`claude-debug/native-n5/g-pixels.py`): at zoom 1 the tiles match the old drawing within 5 of 255; at zoom 0.37, 2.7% of pixels differ by more than 32, all inside dense bands of wires
- Context: the maintainer: "note for future". Not a defect to fix now (note 24's open items).
- Issue: none

### B97. Some core tests fail on Windows: Unix-style absolute paths, and engine programs a test expects beside it

- Reported: 2026-11-22
- Report: found running the touched crates' tests on the Windows test VM for B95 (`native/n5-gate-3`); the same tests fail with B95's change taken out, so they were failing before it
- Context: `plumcraft-host` `manifest::tests::a_uv_project_folder_with_no_pyproject_warns` and `a_uv_run_with_no_project_from_a_folder_with_none_warns` (`/nowhere/at/all` and `/elsewhere` are not absolute on Windows, so the rule judges them); `plumcraft-engine` `register::tests::register_from_uvs_cache_warns_and_from_a_temporary_one_refuses` (register.rs:311); `plumcraft-editor-core` `engines::tests::a_desktop_offers_every_hosting_as_before` (engines.rs:964, the engine programs aren't beside a debug test), `switchboard::tests::an_engine_that_exits_at_once_fails_the_switch_at_once_saying_why` (switchboard.rs:950) and `switching_adding_and_clearing_without_a_window` (switchboard.rs:818). CI is Linux only, so nothing runs these on Windows. Not changed.
- Issue: none

### B92. A sound popped out by a double click on its pin started playing by itself, twice

- Reported: 2026-11-21
- Report: found setting up the sound for the maintainer to hear (`native/n5-gate`)
- Context: opening an Array to Audio value's window by a double click on its output pin, the window played at once twice out of six tries. The app's log says the Play button's `Click` handler ran (`sound: Play/Pause clicked`, then the core's `viewer.audio` state playing, seq 1), with no click of the script's on the button (the pointer was on the pin, outside the new window) and no key sent. With handlers added that log every pointer press, release and key on Play, three more tries didn't play. The core never starts a sound on its own (`viewer_state.rs`: `playing` starts false). Whether the Apple apps or egui do the same was not checked. Cause not found.
- Issue: none

### B91. The Windows app offers Text and Array as a separate process but ships no engine program, so the engine never connects

- Reported: 2026-11-21
- Report: found by the person check (`native/windows/person-check.ps1`) in the N5 gate (`native/n5-gate`): Pick Engines Automatically turned off in Settings, then Engine › Text › Rust · process (mode 2), as a person would
- Context: the core starts a separate-process engine from the program beside the app (`engine_binary`: `plumcraft-text-engine.exe` beside `Plummer.exe`). The Mac app's build copies the engine programs into its bundle (`PLUMCRAFT_ENGINES_DIR`, `project.yml`); the Windows app's build copied only `plumcraft_ffi.dll`, so the core offered the hosting, the person chose it, and the status line stayed at "process (mode 2), not connected". No suite case or test chose a separate-process hosting in the launched Windows app: every one used the in-process engines or the port.
- Verified 2026-11-22 on the Windows test VM: the person check's three runs chose Text › Rust · process (mode 2) and the status line said "mode 2 · process · connected"; in the third, a node ran on it to "HELLO PERSON". The core now disables a hosting whose program is missing, with the reason (another agent's change, merged).
- Fixed in part: 2026-11-21, branch `native/n5-gate`: the app's build copies `plumcraft-text-engine.exe` and `plumcraft-array-engine.exe` beside the app when they are built with the core (`PlumCraft.App.csproj`; the README's build step builds them). Left open: the audio engine is optional and not shipped, and the core still offers a hosting whose program isn't there (it could disable it with the reason, as it does a Python hosting with no Python).
- Issue: none

### B90. The Windows app's Engine menu listed no engines

- Reported: 2026-11-21
- Report: found by the person check (`native/windows/person-check.ps1`) in the N5 gate (`native/n5-gate`), the first launch driven as a person would: the Engine menu showed Reconnect, Attach…, Reload Engine List, Clear Engines, Install Engine…, Cancel Install, then one item with no label and an arrow, whose submenu held the engines
- Context: the core puts the engines in an inline section (`look: "section"`, `submenu: false`, no label: `menu::engine_items`), and the Windows menus drew any item with items as a submenu (`MenuItem.IsSubmenu`). The suite's case "an engine a manifest in the engines folder names is offered" passed against the app because it reads the core's menu model over the port, not what the app draws. Reading the Mac app's `MainMenu.fill`, it draws such an item the same way (an unlabelled submenu); not run on the Mac here.
- Fixed: 2026-11-21, branch `native/n5-gate`: a section's items are put in place (`Menus.MakeAll`, under its label when it has one). Updating the menus in place also paired items with their views by position in an order that put a submenu after its items and counted separators the views don't have; it now uses the order the views are made in (`Menus.Walk`). The person check's Engine menu step lists the engines, the conformance probe from the scratch manifest among them.
- Issue: none

### B55. On the Windows test VM a Plummer process stuck where nothing could end it, and the console session stopped starting programs

- Reported: 2026-11-21
- Report: found while taking N5 slice 4's screenshots (`native/n5-panels`); recorded under the standing "record bugs"
- Context: a screenshot run launched the app (`edit-number`, 15:17) through a one-shot Interactive scheduled task, as every run does. It never wrote its first window to its status file. From then on no program started in the console session: every later scheduled task's process (cmd, PowerShell, the screenshot and input injectors) stayed at its start, and the app's process and the console host and terminal processes launched with it couldn't be ended (`Stop-Process` and `taskkill` reported success or access denied, and the processes stayed). Windows logged a `LiveKernelEvent` (code `1cc`) at 15:31 and an application hang in another app. A program started from the SSH session (session 0) still runs. The VM was not restarted (the brief: never restart it); the stuck processes are still there. Whether the app caused it, or the VM's display driver did and the app was the first to meet it, is not known.
- Looked at after the maintainer restarted the VM: Windows logged `LiveKernelEvent` `1cc` (a kernel resource held too long, a live dump in `C:\WINDOWS\LiveKernelReports\ResourceTimeout-20261121-1520.dmp`) at 15:20, three minutes after the stuck launch, and again at 15:30 and 15:31 with hang reports for two system apps (the Store, the AAD broker). There was no crash or hang report for the stuck Plummer itself, no WerFault and no debugger attached, and the app never wrote its first window to its status file, so it stuck before any window, sheet or dialog of its own was shown, not on a modal sheet and not in a crashed composition thread. Every launch then went through `cmd /c` in a console that Windows handed to Windows Terminal (its default terminal), and Windows Terminal, OpenConsole and conhost started with that launch were among the processes nothing could end. Since the restart every scripted launch runs under `conhost.exe --headless`, so no terminal is started; it did not recur in the seven launches for the suite after the restart (four on ARM64, two on x64, one from the Mac's runner); later the VM stopped answering altogether (no ping, no SSH), and was left so. The dump was not opened (no kernel debugger on the VM). On `native/n5-gate` (2026-11-21, evening, after the maintainer restarted the VM with 10 GB): the person check (`native/windows/person-check.ps1`) ran twice to its end, the app quitting by File › Quit the second time; a minute later SSH accepted connections but sent nothing, ping still answered, and some minutes after that the VM stopped answering ping too. Whether this is B55 again is not known: nothing could be read from the VM. Twice more since: on 2026-11-21 at 22:36 (Mac clock), a minute after scripted double clicks on a pin and as `dotnet test` started; and on 2026-11-22 at 09:35:29, with the VM's emulated TPM turned off by the maintainer, about a minute into the person check, after cargo, the app build and four `dotnet test` runs had all passed without a lost ping. So the TPM is not the cause, or not the only cause. Each of the three came during or soon after a script that drove the app through UI Automation and SendInput; many such runs did not freeze it.
### B39. `bytes_held_elsewhere_are_shared_not_read_again` failed once on the Linux test host

- Reported: 2026-11-20
- Report: recorded by the controller under the maintainer's "record bugs"
- Context: `plumcraft-media` `service::tests`, `crates/media/src/service.rs:1133`, "let go of everywhere: read again": after the test dropped its two handles on the bytes, the service still answered with them. Failed once in the full run for N3 slice 2; passed 300 of 300 alone. Not looked into; whether a worker can still hold the bytes at that moment is the first thing to check.
- Issue: none

### B40. `scrolling_in_past_the_picture_asks_for_detail_once` failed once on the Mac under the full parallel run

- Reported: 2026-11-20
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: `plumcraft-ui::kittest_viewer_tools`. Failed once in the full parallel nextest run for N4 slice 2 on the Mac; passed 20 of 20 alone. The message was not captured. Not looked into.
- Seen again (a neighbour): 2026-11-21, `kittest_viewer_tools::a_click_pins_the_probe_and_the_command_does_the_same` failed once in a full parallel run on the Mac (the person gate branch's core work) and passed alone; the message was not kept.
- Issue: none

### B54. The Windows app ended once with a stowed exception during the conformance suite's read half

- Reported: 2026-11-21
- Report: found by the gate of N5 slice 4 (`native/n5-panels`); recorded under the standing "record bugs"
- Context: the first run of the read half against slice 4's build on the Windows test VM: the app process ended (exit `0xC000027B`, a stowed exception; Windows Error Reporting names `CoreMessagingXP.dll`, `combase.dll` and `E_INVALIDARG`) around the case "a watched picture arrives as picture", while pop-out windows the core opened for the earlier cases were being closed by `graph.new`. Nothing reached the app's own handler or log. Pop-out and panel windows now drop what comes for them after they close (a value read, a viewer's page, a report), every viewer's update catches and logs its own faults, and `PLUMMER_TRACE` logs every exception as it is thrown. The run of both halves after these changes passed, and about forty scripted launches for screenshots ended normally; the x64 run and repeated runs were not taken (B55); the cause was not found, so this stays open. After the VM's restart, with every exception logged as thrown (`PLUMMER_TRACE`): the suite four times on the final ARM64 build, twice on x64 and once from the Mac's runner, every one to the end with the app stopped by its PID; no stowed exception and no app exit. Not reproduced.
- Issue: none

### B52. `CoreFlow.ManyRequestsInFlightAndWakesCoalesce` failed once on the Windows test machine: "busy: the document's engine is still connecting"

- Reported: 2026-11-21
- Report: found by the gate of N5 slice 3 (`native/n5-editing`); recorded under the standing "record bugs"
- Context: `native/windows/tests/PlumCraft.Kit.Tests/CoreTests.cs:149`, the Windows kit's test that sends 500 requests at once to a core just opened. In one of four `dotnet test` runs on the Windows 11 ARM64 test machine (40 of 41 passed) the core refused one with "busy: the document's engine is still connecting": the C ABI core holds at most 256 requests for a document whose engine hasn't connected yet (`MAX_HELD`, `crates/ffi/src/core.rs`), and the test doesn't wait for the engine before sending them. The run before and the two after passed 41 of 41. Not changed: the test or the limit, whichever is wrong, is for the maintainer.
- Decided (the maintainer): "Make the test wait". On `native/n5-gate` the test sends `scene.get` first, which the core holds until the document's engine connects (`ping` is the core's own and isn't held), and only then the 500 requests. On 2026-11-22 it passed in three full `dotnet test` runs on the Windows test VM (TPM off); left open for the maintainer to close.
- Issue: none

### B111. On the iPad, after a number typed into a face's field, the app never reports idle (XCTest waited 60 s on every step)

- Reported: 2026-11-21
- Report: the controller's device run of the iPad UI tests (the maintainer present; built from the main checkout, `build-device/test`): `testInlineEdits` passed but took 863 s; "App animations complete notification not received" 14 times in the run
- Context: the log shows the first field (text) typed into and Done tapped normally; then `7` typed into the `count` row's number field (the numbers-and-punctuation keyboard), and from the tap on `row-editor-done` that follows the app never reported idle again: every later step waited 60 s first. So something starts animating or redrawing with the number field's editor and doesn't stop after it closes; a battery defect on a device too.
- Looked into (branch `native/n4-decided-2`, no device use): **not reproduced** on an iPad Pro 11-inch (M5) simulator booted headless (the software number keyboard shows, as on the device), in portrait, with the status read from the file and again from the accessibility element as the device read it: `testInlineEdits` passed in 21 s and 22 s, no idle wait. What runs without end in the iPad's canvas: only a running face's wash pulse (`LayerCanvas`, `repeatCount = .infinity`, while a face shows running); the pan momentum's display link stops itself; the wire tiles have no timer; the keyboard's lift (`FieldEditorHost.keepAboveKeyboard`) animates the viewport once per keyboard frame change. Candidates on a device: a face left showing running (its pulse), or the keyboard lift and the keyboard's frame notifications feeding each other at the device's keyboard height. Needs the device: re-run with the app reporting what animates (a pulse, the momentum link, a viewport animation) in its status.
- Issue: none

### B112. On the iPad, the table viewer's output pin double-tapped did not pop it out (`testEveryViewerByATap`)

- Reported: 2026-11-21
- Report: the controller's device run: "timed out waiting for tab popped out in table.grid", "tab: the pop-outs were []", then no `popout-done`
- Looked into: **not reproduced** on the simulator (status by file and by element): the test passed in 28 s and 30 s, the table first. The test predates today's pop-out and grid-window changes, but it passes against them here; the device ran the main checkout's build, not this branch. The first viewer is the one a double tap right after launch hits: on a device the first double tap may land before the faces' parts are measured (as B88 on the Mac), or the system takes it as two taps (the test retries once after 8 s). Needs a device re-run.
- Issue: none

### B113. On the iPad, `testTouchGestures` failed: the pinch did not zoom, the palette was not dismissed, the marquee selected nothing, no Select button

- Reported: 2026-11-21
- Report: the controller's device run: "timed out waiting for zoomed in by the pinch" (zoom stayed 1), "the palette dismissed", "the marquee selected", and `GestureTests.swift:122` and `:226` (no "Select" button)
- Looked into: **not reproduced** on the simulator: passed in 39 s and 40 s. The later failures follow the first (the zoom never changed, so the points the test aims at were elsewhere). The Select button is the toolbar's Select mode (`canvas.selectMode`, N4 slice 3); a device window narrower than regular width puts it in the More menu, where the test doesn't look. Needs a device re-run, with the window's width class and the toolbar's items in the failure's attachments.
- Issue: none

### B114. On the iPad, `testWindowsAndNames` failed: not inside the group, no `crumb.0` button

- Reported: 2026-11-21
- Report: the controller's device run: "timed out waiting for inside the group", no `crumb.0` button (`GestureTests.swift:397`)
- Looked into: **not reproduced** on the simulator: passed in 20 s both times (B60's step, Show on Face after a hidden value is set, passed too). Entering the group is a double tap on its face; the breadcrumb appears only inside. Needs a device re-run.
- Issue: none

### B60. `testWindowsAndNames` fails on the iPad: Show on Face not found after editing a hidden value

- Reported: 2026-11-21
- Report: the controller, from the iPad UI test run on the maintainer's iPad (portrait, 834 by 1210 points, the dev branch at `50b8aec`)
- Context: `native/apple/PlumCraftPad/UITests/GestureTests.swift:359`. Four of five UI tests passed on the device (`testCompactWindowsByATap`, `testEveryViewerByATap`, `testInlineEdits`, `testTouchGestures`); `testWindowsAndNames` failed. After the parameters popover opened on node `w`, the field `edit.count` was found, tapped and typed into (`5\n`; the status then said "Set Value"), and `app.buttons["face.count"].tap()` failed with "Failed to get matching snapshot: No matches found for Elements matching predicate '"face.count" IN identifiers'". The test passes in the simulator.
- Looked into (2026-11-21, branch `native/n4-wire-hitches`): **not reproduced, cause not found.** In an iPad Pro 11-inch (M4) simulator in portrait, booted headless so the software keyboard shows as on the iPad, with the status read from the accessibility element as on a device, the test passed every time: at the default text size, at the three largest standard sizes and at Accessibility Large. The popover opens above `w`, in the space between the toolbar and the node, and shows about three rows; while the count field is edited UIKit keeps its row in view, and after Return and the list's reload the row and its Show on Face button are still there. Nothing in the popover's code (`PlumCraftPad/App/Parameters.swift`) dismisses it on Return or changes on a device. Ruled out in the simulator: the software keyboard, a larger text size, the status element. Not ruled out: timing on the device (the test tapped `face.count` the moment the status said "Set Value", while the keyboard was going and the list reloading; every other step of the test waits for its element), and anything the device does that the simulator doesn't. **The test now waits for the button (10 s), looks for it in the list both ways if it isn't in view, and attaches a screenshot (`b60-face-count-missing`) and the app's element tree (`b60-element-tree`) when it still can't find it,** with screenshots while the keyboard is up (`n4s3-ui-count-typing`) and once the value is set (`n4s3-ui-count-set`). No app change: no app defect was shown. To confirm: `native/apple/PlumCraftPad/device.sh ui` on the iPad; if `testWindowsAndNames` passes, the cause was the tap's timing; if it fails, the two attachments show what the popover held.
- The device run after it (2026-11-21, the controller, the maintainer present; the main checkout's build): `testWindowsAndNames` failed again, but earlier, at entering the group (B114), before the step B60 is about, so this run says nothing of B60. Five tests: 2 passed (`testCompactWindowsByATap`, `testInlineEdits`, the latter in 863 s, B111), 3 failed (`testEveryViewerByATap` B112, `testTouchGestures` B113, `testWindowsAndNames` B114), 11 failures, 1,185 s. On an iPad Pro 11-inch (M5) simulator in portrait (branch `native/n4-decided-2`) all five passed twice, the status read from the file and from the accessibility element. Still open: a device re-run decides it.
- Issue: none

## Closed, not reproduced

Closed without a fix: the maintainer decided they need no more work unless they are seen again.

### B46. A Cancel Run was not honoured once in the egui editor (unconfirmed)

- Reported: 2026-11-21
- Report: found by the egui audit; recorded under the standing "record bugs"
- Context: a 5 s Slow node finished `done` at 5,008 ms after Cancel Run; seven later attempts all cancelled within 2.4 s, and the first run's log was overwritten. Seen once, on the Linux test host. `claude-debug/egui-audit.md`, defect D9.
- Tried (2026-11-21, egui parity, on the Mac): 510 cancels of a 1.5 s Slow node at delays from 0 to 1,000 ms after `run.start`, nothing cached: 150 in the egui editor (the command and the menu's invoke alternately), 120 with a run already going when `run.start` superseded it (Auto-Run's or another `run.start`), 90 through egui's control dispatcher (`ui.invoke run.cancel`, `ui.waitRun`), and 150 through the C ABI core (`command.run` and `ui.invoke {by: person}`). Every one ended `cancelled`. In the audit's sequence the cancel came after 40 frames, the first GPU picture of that process on a software renderer, `ui.inspect` and `menu.get`; if those took over 5 s, the node had finished before Cancel was sent, which would explain it. Scratch drivers: `claude-debug/egui-parity/b46/`.
- Closed: 2026-11-21, not reproduced, by the maintainer ("Close", asked whether to close it or rerun the audit's sequence on the Linux test host first). 510 attempts, every one cancelled; the likely explanation is the one above: the audit's Cancel came after 40 frames, the process's first GPU picture on a software renderer, `ui.inspect` and `menu.get`, and if those took over 5 s the node had already finished before Cancel was sent.
- Issue: none

## Fixed

### B95. The copied-manifest warning shows a Windows path with its `\\?\` prefix

- Reported: 2026-11-22
- Report: seen in the engine cell's tooltip on the Windows test VM (`native/n5-gate-2`): "uv run --project . names \\?\C:\…\engines, with no pyproject.toml…"
- Context: the core canonicalises the manifest's folder (`std::fs::canonicalize`, which on Windows gives a verbatim path) and put it in the words a person reads.
- Fixed: 2026-11-22, branch `native/n5-gate-3` (the maintainer: "yes"): `plumcraft_proto::programs::shown` (and `shown_str`) take the prefix off a path for display (`\\?\C:\x` → `C:\x`, `\\?\UNC\server\share` → `\\server\share`, anything else unchanged), used in the copied-manifest warnings (`crates/host/src/manifest.rs`), a duplicate engine id's problem (`crates/editor-core/src/engines.rs`, the one `problems.push` for an id already loaded) and `register`'s uv-cache warning and refusal (`crates/engine/src/register.rs`); paths used for file operations keep it. Tests: string cases on every platform, a canonical temporary folder on Windows, and the copied-manifest warning on Windows names the folder without the prefix. Seen in the app: the engine cell's tooltip names `C:\…\engines`.
- Issue: none

### B94. The Windows palette's kinds had no names for UI Automation

- Reported: 2026-11-22
- Report: found by the person check (`native/windows/person-check.ps1`, branch `native/n5-gate`): searching the palette for "Text" left one list item named "Text" (the family's heading) and every kind's row unnamed, so the check, or a screen reader, could not tell the kinds apart
- Context: each kind's row is a `Grid` of text blocks, which UI Automation shows as a list item with no name.
- Fixed: 2026-11-22, branch `native/n5-gate`: each row is named by its label and note (`Editors.cs`), and a family's heading is marked a heading. The person check's third run on 2026-11-22 picked "Text" from the palette by its name and passed every step it covers but one (its own reading of a field's value, since fixed).
- Issue: none

### B93. A graph opened from the Windows app's command line is titled Untitled

- Reported: 2026-11-21
- Report: found setting up the sound for the maintainer to hear (`native/n5-gate`): `Plummer.exe --root <scratch> <graph>.json` showed the graph, and the window's title, its status file's `title` and UI Automation all said "Untitled - Plummer"; the same graph opened by File › Open is titled by its file
- Context: the window titles itself from the document's state in the feed (`DocumentState.Name`, else `Path`). The core announces a document's state when it opens it; the command line's graph is opened (`document.open {path}`) before its window's model follows the feed, so that announcement was missed and the window waited for the next. The Mac app titles its windows from NSDocument's file; egui from the core's title; the core's state was right.
- Fixed: 2026-11-22, branch `native/n5-gate-2`: the model reads the document's state once from `ui.inspect` when it starts, as it already read the engine's (`DocumentModel.RefreshDocument`); a kit test (`AGraphOpenedBeforeItsModelIsKnownByItsFile`) and a person-check step (the saved graph opened from the command line is titled by its file) pin it. The person check passed it three times.
- Issue: none

### B110. A node working on an engine that is killed mid-node kept its progress on its face for good

- Reported: 2026-11-21
- Report: found by the person check's engine half (decision 10, branch `native/n4-decided-2`): the installed engine's process killed while `text.slow` was at 40%
- Context: `crates/session/src/run.rs` `apply_reply`. With more than one engine in the document (the built-in text engine and the installed one), the coordinator answered the run with the engines that were still there and the run's error; the slow node had no final record and was in the reply's nodes, so its state stayed `progress` (its fraction too) after the run ended: the face showed a node still working that nothing would ever finish. `RunState::fail` (the document's only engine gone) unmarks such nodes, but it is not called while another engine is connected.
- Fix: when a run's reply comes, a node still marked working (started, progress, preview) in a failed run is in error with the run's error (`engine` kind: "the engine stopped (signal 9); restarting"), and one only queued is unmarked, whatever the reply says of it.
- Verified: `run::not_run_tests::a_node_still_working_when_a_failed_run_ends_is_in_error`; the person check's F35 step on the launched Mac app (the face goes from progress to error, "engine error: the engine stopped (signal 9); restarting", the run ends 1.6 s after the kill, the engine comes back).
- Fixed: 2026-11-21
- Issue: none

### B87. An engine manifest the core can't read is skipped without a word to the person

- Reported: 2026-11-21
- Report: found by the person check (branch `native/n4-person-gate`)
- Context: a manifest with an id the core refuses was left out of the Engine menu silently at start; only Engine › Reload Engine List's reply carried `problems`, and the Mac shows no reply.
- Fix (decision 11, "fix B86 and B87 now"): what the last read of the manifests couldn't use or warned about is kept with the engines (`engine.list`'s and `ui.inspect`'s `engine.manifestProblems`, one line a manifest), put in the status line of a document opened after the read once its startup has settled (`engine manifests: …`, or `engine manifests: N problems; the first: …`), and in the log as before; Reload Engine List updates both (`Document::note_manifest_problems`). `docs/interface.md` 16.6.
- Verified: `engines::tests::an_unreadable_manifest_is_reported_to_the_person`, `engines_install::an_unreadable_manifest_is_said_to_the_first_document` (C ABI: the feed's document state and `ui.inspect`); the person check: a manifest with a hyphenated id in the scratch engines folder is in `manifestProblems` and the Mac window's banner at launch.
- Fixed: 2026-11-21
- Issue: none

### B86. A node's Run on menu names engines by their ids, not their names

- Reported: 2026-11-21
- Report: found by the person check (branch `native/n4-person-gate`)
- Context: `crates/editor-core/src/menu.rs`, the node context menu's Run on: an installed engine was offered as `person_text`, the built-in one as `text`.
- Fix: Run on names each engine as the Engine menu does (its choice's label; an engine with no choice keeps its id); the command and its params are unchanged (`menu::Parts` carries the engine choices; egui's canvas passes them).
- Verified: `menu::tests::run_on_names_engines_by_their_labels`; the person check picks Run on › Person Check Text on both nodes of the launched Mac app.
- Fixed: 2026-11-21
- Issue: none

### B84. A scripted launch that shows the Mac's Save or Open panel changes the person's defaults, or saves into their folders

- Reported: 2026-11-21
- Report: found by the person check's first runs (branch `native/n4-person-gate`)
- Context: AppKit's panels run out of process and keep their folder in the app's defaults domain, which can't be pointed elsewhere (B50).
- Fix (decision 7, "scripted launches get their own bundle id"): `native/apple/PlumCraftMac/scripted-app.sh` makes a copy of the built app under `com.swapwizard.plummer.mac.scripted` (a clone, signed again ad hoc for this Mac only) and, when the run ends, removes what the system kept for that id (defaults, saved state, recent documents, caches, HTTP storage, WebKit, the temporary folder's state) and unregisters and removes the copy; the app refuses `--scratch-state` under any id that is not a scripted copy's (`ScratchGuard.bundleRefusal`); `conformance.sh`, `person-check.sh` and the Mac `bench.sh` run the copy, and every one lists the person's home top level, `~/workspace` top level, `~/Documents` and `~/Desktop` before and after and fails loudly when anything came or went (`library-check.sh home`), besides the person's Library for both their ids. The panels are set before they show (`GraphDocument.prepareSavePanel`, the core's file asks), read from outside through accessibility (the panel's own name field and folder) and cancelled: **no real Save or Open is pressed** (the maintainer, 2026-11-21: "1,2 as you say"); the press is behind `PERSON_PRESS_SAVE=1`, which no script sets and `person-check.sh` refuses, and `may_press` (tested, 12 cases, each run) refuses unless both fields are a scratch folder's.
- Found on the way: the scripted copy's first save through the real panel, set after the panel showed, landed in the person's `~/Documents` as `Untitled.json` (the run's home listing caught it; deleted by the controller); that is why nothing is pressed. And the scripted copy, noting a recent document, stopped in Swift's isolation check: `PlummerDocumentController.maximumRecentDocumentCount` is read by AppKit from a queue of its own; it is `nonisolated` now, and the person check checks the app is alive after a recent document is noted.
- Verified: the person check, three runs in the Mac gate: the person's build refuses a scripted launch; the save panel opens on the scratch folder with the name (from the panel itself); the open panel opens on the scratch folder; the person's Library for both ids, the home listings and the engines folder unchanged; the scripted id's Library as before after the cleanup.
- Fixed: 2026-11-21
- Issue: none

### B83. The control channel acts on the first document only, and ignores a request's `doc` without saying so

- Reported: 2026-11-21
- Report: found by the person check (branch `native/n4-person-gate`)
- Fix (decision 8, "the control channel honours the document a request names"): the control server passes a request's top-level `doc` on (`ControlRequest.doc`); the C ABI core answers for that document (the first with none), and refuses one that isn't open (`code: "unknown_doc"`) or isn't a number (`bad_params`); the egui editor, which serves one document, refuses any `doc` (`unknown_doc`). `docs/control-protocol.md` and `docs/shell-interface.md` 4.1. No conformance case: a driver over the port can't open a second document.
- Verified: `server::tests::auth_first_then_requests_reach_the_ui`, `control_port::the_control_port_honours_the_document_a_request_names`, egui `control::a_request_naming_a_document_is_refused_not_answered_for_this_one`; the person check's engine half reads its own window's scene by `doc`.
- Fixed: 2026-11-21
- Issue: none

### B81. Opening File › Open Recent in a scripted launch of a newly built Mac app rewrites the person's recent-documents list

- Reported: 2026-11-21
- Fix: falls out of B84 (decision 9, "leave Open Recent empty in scripted launches without touching the person's list"): a scripted launch is the scripted copy, whose recent documents are its own list (it starts empty, the script removes it at the end).
- Verified: the person check: the saved graph is in the copy's own recent list and opened from it; the person's `com.swapwizard.plummer.mac.sfl4` byte for byte unchanged (`library-check.sh`, size, time and hash). Not seen: Open Recent's menu items, which AppKit fills only while the menu tracks (the check never takes the pointer), so the check opens the list's entry as the item does (a SEE line).
- Fixed: 2026-11-21
- Issue: none

### B76. The iPad bench leaves each Instruments trace's working file in the temporary folder

- Reported: 2026-11-21
- Cause: the raw kernel trace (`instruments*.ktrace`) is written by Xcode's `DTServiceHub`, a service launchd starts, into the per-user temporary folder (`getconf DARWIN_USER_TEMP_DIR`), never into the TMPDIR given to xctrace, and is left there (still open in DTServiceHub, which lingers). Shown without a device: a three-second System Trace of `/bin/sleep` with TMPDIR set left a 386 MB `instruments….ktrace` there and nothing in xctrace's folder (`claude-debug/n4-decided-2/b76-probe.sh`).
- Fix: `native/apple/ktrace-sweep.sh` lists the raw traces there before a recording and removes every new one once its tables are exported, or on any way out (the EXIT trap); `PlumCraftPad/bench.sh` and `PlumCraftMac/bench.sh` use it.
- Verified: on the Mac, a recording with the sweep around it as the scripts use it: one new 257 MB raw trace, removed, none left (`claude-debug/n4-decided-2/b76-verify.sh`). Not verified: a device trace (no device use allowed); the space of a removed file comes back when DTServiceHub closes it.
- Fixed: 2026-11-21
- Issue: none

### B56. The sound case races a shell that reports a sound's end: under emulation the 64 ms sound ends between two steps

- Reported: 2026-11-21
- Fix (decision 15): the case sets `loop` before `play`, so no shell reports the sound's end while the case runs, however slow (the Apple apps' player and muted clock report no end while looping); the case still checks play, seek, level, loop and pause.
- Verified: the core's document, the C ABI and the egui editor (`cargo nextest`, the conformance tests), the Mac app (`conformance.sh`, 57 of 57). Not run: Windows under emulation.
- Fixed: 2026-11-21
- Issue: none

### B80. The Apple apps' Engine menu put every engine in a submenu with no title

- Reported: 2026-11-21
- Report: found by the person check, the floor re-walk's first launch as a person (branch `native/n4-person-gate`)
- Context: the core's Engine menu ends with a section (`look: "section"`, not a submenu, no label) that holds every engine (Text, Audio, Array, the craft engines, the installed ones). The kit's `MenuItem.isSubmenu` counted any item with items as a submenu, so the Mac drew the engines under a blank item at the foot of the Engine menu, and the iPad's menu bar and More menu did the same. A person saw Reconnect, Attach…, Reload Engine List, Clear Engines, Install Engine… and Cancel Install, and one unlabelled arrow: the engines, installed ones included, were there only to someone who opened it. Part of what B71's "no engines available" saw.
- Fix: `MenuItem.isSection`; a section's items are drawn inline in the menu they are in, under a heading when it has a label (Mac menu bar and context menus, the iPad's menu bar and More menu).
- Verified: the person check lists the Engine menu as AppKit validated it: every engine by name at the menu's level, the installed one with them, no untitled item (`native/apple/PlumCraftMac/person-check.sh`). The iPad's drawing was built for the simulator and a device (iOS Release) but not looked at.
- Fixed: 2026-11-21
- Issue: none

### B82. The Mac's Install Engine… form cut its hint off

- Reported: 2026-11-21
- Report: the person check's picture of the Install sheet (branch `native/n4-person-gate`)
- Context: the repository field's hint, "owner/repo or https://github.com/owner/repo", was wider than the field (280 points), so a person read "…github.com/owner/".
- Fix: a form's fields are as wide as their longest hint needs, up to 440 points, and every field carries its whole hint as its tooltip (`native/apple/PlumCraftMac/App/Asks.swift`).
- Verified: the person check's picture of the sheet (`claude-debug/native-n4-person-gate.md`).
- Fixed: 2026-11-21
- Issue: none

### B72. Starting a manifest engine as a process that exits at once takes 30 s to fail

- Reported: 2026-11-21
- Report: found by the controller checking B71: `engine.switch` to an engine whose program exits immediately (its Python module is not installed) got no reply for 30 s as a process; over HTTP the same engine failed in 0.2 s with "the engine exited before listening".
- Context: `crates/editor-core`, the process start of an engine a manifest names.
- Cause: the process host (`crates/host/src/process.rs`) saw the child exit before it connected within milliseconds, but treated it as a crash: it said `engine_down` and restarted it, five times in about 3 s, then stopped. `ProcessHost::wait_ready` kept waiting while the engine was down whether or not anything would restart it, so the editor core's start (`connect_external`, and `start` for the built-in engines' process mode) waited out its whole limit (60 s for a manifest's engine, uv may build an environment first; 10 s for the text and audio binaries), longer than the control channel's 30 s, and then made a link to a dead engine: the switch "worked". The child's stderr was kept, but on its own thread with nothing waiting for its last lines. Over HTTP the host already failed at once, but its stderr was inherited, so the reason had no last words.
- Fix: 2026-11-21, branch `fix/b72-b37-b38`. An engine that exits (or can't be started) before it has ever connected is not restarted: the host gives up at once, with a reason that says how it exited and ends with the last five lines of its stderr (after waiting up to 1 s for the pipe to close), and `ProcessHost::wait_started` returns that reason as an error. `wait_ready` no longer waits for an engine supervision has given up on (this one, or one that crashed five times in a minute). `hello`, `session.confirm` and `schema` during a start give up as soon as the child exits, not after 10 s. One the host stops for not connecting within 10 s is restarted as before. The editor core's starts (a manifest's engine, the built-in engines and packs in process mode) and the CLI use `wait_started`, so `engine.switch` fails with the reason. An HTTP engine's stderr is now copied through as before and its end kept, so its "exited before listening" carries the same last words. `docs/interface.md` 16.3 says so.
- The folder question: `docs/interface.md` 16.6 already says `cwd` is relative to the manifest's real folder (a link is followed back to the project), and `plumcraft_host::manifest::load` does that, so it is not a second bug. The installed OpenCV manifest (a link into its project) started fine from a folder that wasn't its project, as a process and over HTTP; a copy of it in another folder gave exactly the report's "No module named opencv_engine", in both hostings (`claude-debug/b72/opencv_cwd.sh`). The report's manifest was very likely a copy, not the link. `--project .` is uv's own argument and resolves against that `cwd`.
- Follow-up: 2026-11-21, branch `fix/manifest-warn-python-clock` (the maintainer: "1,2,3 yes"): such a copy now loads with a warning, and its failed start ends with what to do about it (`docs/interface.md` 16.6).
- Verified: new tests, a manifest in a scratch folder whose command prints its folder and "No module named opencv_engine" to stderr and exits 1: `engine.switch` as a process failed in 10 ms and over HTTP in 26 ms, each with the exit status, the last words and the manifest's folder (`switchboard::tests::an_engine_that_exits_at_once_fails_the_switch_at_once_saying_why`). The same test against the old code waited 60 s and then the switch succeeded with a dead engine. A text or audio engine binary that exits at once fails the start within 3 s in both hostings (`engines::tests::a_built_in_engine_binary_that_exits_at_once_fails_the_start_at_once`). `mode2_an_engine_that_cannot_start_is_down_not_hung` now asserts the reason, that the start fails within 2 s and that the program was started once. On the Linux test host all of these passed 60 of 60 (30 quiet, 30 under load). Not checked on Windows (its variant of the switch test uses `cmd`).
- Fixed: 2026-11-21
- Issue: none

### B37. `fast_never_session_too_big_and_setup_bound_results_are_not_persisted` failed once on the Linux test host

- Reported: 2026-11-20
- Report: "record bugs"
- Context: `plumcraft-engine::persist`. Failed once in a full parallel run while three other builds were running on the host; the message was not captured. Passed 3 of 3 alone. Not looked into.
- Seen again: 2026-11-20, in the full run for N3 slice 2 with nothing else building. It fails at `crates/engine/tests/persist.rs:166`, the "too big" case: a 60 ms node that makes 32 MB was persisted, where the test expects it not to be worth it. Passed 40 of 40 alone. The test's own comment says the margin is a matter of timing.
- Seen again: 2026-11-20, on the Mac in N4 slice 2's gate, in the Python suite's test of the same name (`python/tests/test_persist.py`) while cargo was running in parallel. Passed 10 of 10 alone.
- Seen again: 2026-11-20, on the Linux test host in N4 slice 2's full run (the Rust test), together with B38. Both passed 10 of 10 alone afterwards.
- Note: 2026-11-21, B49 found the Python test's cause: the work is measured on the wall clock, and allocating the big array can stall for over 150 ms while the kernel compacts memory, which makes a 60 ms node look worth persisting. The Rust test probably fails the same way; it has no virtual clock to pin the work, so its fix needs an injectable clock in the Rust executor or a much bigger margin. Not checked.
- Cause: as B49's. The test measures a node's work on the wall clock, and the "too big" case (about 32.18 MB of outputs, worth persisting after 321.8 ms of work) gave the rule 262 ms of room over a 60 ms node; when allocating the 32 MB array stalls past that on a busy host, the 60 ms node rightly looks worth persisting. Not reproduced on the Linux test host by load alone: under B49's load (32 niced spinners and 6 memory churners, load average 30 to 43, about 220,000 compaction stalls system-wide over the runs) the old test passed 30 of 30, and 30 of 30 quiet. The host's transparent huge pages are `madvise` only and Rust's 32 MB `malloc` never asks for them, so the stall that hit numpy doesn't hit this allocation in the same way; what stalled it in the full gate runs (the gate's own builds and 1,500 tests in parallel) wasn't captured. The rule stays as it is (the maintainer: "Leave the rule").
- Fix: 2026-11-21, branch `fix/b72-b37-b38`, the smallest SDK change: `plumcraft_engine::clock` (a `Clock` trait, `now` and `sleep`; `RealClock`, the default; `VirtualClock` for tests) and `Engine::with_clock`; the executor measures a node's work on the engine's clock. No behaviour change in production. The persist tests' node sleeps on the clock it is given, and the not-persisted cases run on a virtual clock, on which a node's work is exactly the time it sleeps. A new test, `too_big_is_weighed_against_the_work_measured`, pins the rule either side of its limit: 32.18 MB after 300 ms of work is not persisted, after 345 ms it is, and a node that sleeps no time on a virtual clock did no work, whatever its allocation took.
- Verified: on the Linux test host with a preloaded shim that makes every `malloc` of 16 MB or more wait 300 ms (`claude-debug/b72/stall.c`, `linux_stall.sh`): the old test failed 30 of 30, every time at `persist.rs:166` ("too big"), and the new file passed 30 of 30. Without the shim, old and new both passed 30 of 30 quiet and 30 of 30 under the load above. On the Mac, the engine crate's 72 tests and the full gate.
- Fixed: 2026-11-21
- Issue: none

### B38. `memo_reuses_unchanged_nodes` failed once on the Linux test host

- Reported: 2026-11-20
- Report: "record bugs"
- Context: `plumcraft-text::conformance mode2_process`, `crates/text/tests/conformance.rs:390`, "off really runs the slow node". Failed once in a full parallel run while three other builds were running on the host. Not looked into.
- Seen again: 2026-11-20, on the Linux test host in N4 slice 2's full run, together with B37. Passed 10 of 10 alone afterwards.
- Cause: a timing race in the test, not a persist decision and not the process engine. The case ends by running the graph with `memo: off` and asserting that it took at least 300 ms, the slow node's sleep. Its stopwatch started after `run` returned, that is after the engine's acceptance had come back, but the engine starts the graph before that acceptance reaches the test. Any lateness in the acceptance (a busy host, where the reply waits for a thread to be scheduled) was taken off the 300 ms, so a run that really slept 300 ms could measure less. Every assertion before it (the states, the plan keys) had passed: the node did run.
- Fix: 2026-11-21, branch `fix/b72-b37-b38`: the stopwatch starts before the run is sent, so a reply can't come back sooner than the slow node's own 300 ms. No retry, no wider margin.
- Verified: on the Mac, with a 60 ms sleep put between the acceptance and the stopwatch (a late acceptance), the old timing failed in all six hostings (in-process, coordinated, process, Python process, HTTP, Python HTTP) at exactly "off really runs the slow node", and the new timing passed in all six with the same sleep. Not reproduced naturally: on the Linux test host the old test passed 30 of 30 quiet and 30 of 30 under load, as did the new. What would settle it further: a failure of the new test, which would mean the slow node returned early.
- Fixed: 2026-11-21
### B53. The C# conformance runner passed a `holds` step on its first reply alone

- Reported: 2026-11-21
- Report: found reading the runner against `crates/editor-core/src/conformance.rs` in N5 slice 4 (`native/n5-panels`)
- Context: the suite's `holds: {pointer: want}` step form (with `for`, added for B43: a window the shell closed by itself a moment after it opened) asks again for that long and every reply must match. The Swift and Rust runners do; the C# runner, ported before the form existed, ignored `holds` and passed such a step on its first reply, so "the shell never closes what the core opened" was not checked by the C# runner against the Windows app in slices 2 and 3.
- Fixed: 2026-11-21, branch `native/n5-panels`: `holds` and `for` as the other runners have them (`native/windows/src/PlumCraft.Conformance/Runner.cs`). The Windows app passes the three such steps.
- Issue: none

### B73. The Mac app's forms put every label above the wrong field, the first one clipped

- Reported: 2026-11-21
- Report: "The attach dialog is meh", with a picture: "Address" cut off at the top, "Token" above the address field, the token field unlabelled.
- Cause: `Asks.swift` added all the labels to a stack, then all the fields, in a frame sized for the fields alone. Every form the core asks the Mac for (Attach, Install) was drawn this way. Nothing checked what a form looks like: a scripted ask shows no sheet.
- Fix: a form is a grid, each label right-aligned beside its field on its baseline, sized to fit, with the fields in tab order.
- Verified: the same grid in a system alert drawn to a picture (`claude-debug/b73/form.swift`), labels beside their fields; Mac conformance still passes. Not seen in the app's own sheet by the controller.
- Fixed: 2026-11-21
- Issue: none

### B71. The native apps can't install an engine and don't offer the engines installed

- Reported: 2026-11-21
- Report: the maintainer, on first using the Mac app (Plummer) by hand: "Install engine is greyed out, no engines available. WTF?" They had been told twice that the Mac build could install an engine; it could not.
- Context: Engine › Install Engine… was disabled in Plummer with "this editor can't install engines", and the Engine menu offered none of the engines in `~/.config/plumcraft/engines` (DuckDB and OpenCV, installed with the egui editor). Below the floor: the egui editor installs and offers them.
- Cause: the installer, the manifest reader and the decision to offer installing were in the editor core already (since N1 slice 2), but only the egui application wired them: `apps/plumcraft-editor` called `load_manifests` at start, gave the document a Reloader and an Installer, and `EditorApp::with_installer` set `can_install`; the egui shell's own frame loop followed the install (`poll_install`). The C ABI's core did none of these: it never read the engines folder, had no Reloader (so Reload Engine List did nothing but say "this editor has no engine manifests to read"), no Installer, and never set `can_install`, whatever the platform declared. A test asserted the refusal (`attach_asks_with_the_cores_form_and_install_is_refused_here`). So whether a shell could install was decided by which shell it was, not by the platform: a breach of "the shell decides nothing".
- Fixed: 2026-11-21, branch `fix/b71-native-engines`: the core wires it for every shell. `Launcher` gains `folder` (the engines folder: `dirs.engines`, else `$PLUMCRAFT_ENGINE_FOLDER`, else `~/.config/plumcraft/engines`, the folder every editor shares) and `uv`; `Launcher::load_manifests`, `reloader` and `install_setup`; `Document::offer_installs` gives a document the Reloader and, where `install::refusal(caps)` allows (the platform declares `subprocess` and `python`), the installer, else disables Install Engine… with the reason ("This device can't install engines: it can't start programs" on the iPad). The C ABI's core reads the manifests on its thread at start (`engines.manifests` and the folder) and offers each document what any document read again (`Document::follow_manifests`). `control::step` follows the install, in every shell. The installer is whole in the core (`install::install_engine`): uv found as a manifest's is (the login shell's PATH and the usual folders, since an app from the Finder has none; a miss says where it looked), `register --folder <engines folder>`, each step in a process group of its own with the search PATH and a 30-minute limit, what it is doing (with uv's latest line) as `engine.installing`/`installStatus` and the `document` domain's `installing`, and `engine.install.cancel` (Engine › Cancel Install), new, which stops it and what it started. The egui shell lost its own Reloader and install loop (`EditorApp::with_launcher` takes the core's). Plummer shows "Installing an engine…" in its engine cell while one runs. Verified by: core tests with a stand-in uv (install, register into the folder given, uv's failure, cancel within 5 s, a step past its limit, uv missing), the C ABI tests (a desktop platform's manifests offered from the start, Install Engine…'s form, an install, a second document offered it, a cancelled install; the iPad's platform refused with the reason and an attached manifest engine still offered; bad config fields refused), two new conformance cases on every driver (the core's document, the C ABI and its port, egui, the Swift kit and its port, the Mac app: 55 of 55), and a real install through an ad hoc signed, hardened Plummer launched as the Finder would (a bare environment), on scratch: `seantrue/plumcraft-duckdb-engine` installed and registered in 7.7 s, started as a process (a Homebrew Python, a separate process, so library validation doesn't apply), and a SQL Query node gave 42 (`claude-debug/native-engines-install.md`).
### B70. The Mac app could not make a new graph: File › New did nothing, and a launch showed only the Open panel

- Reported: 2026-11-21
- Report: "I can't create a new graph? Only open an old one?"
- Context: the maintainer's first launch of the Developer ID build of the dev branch. With no file to open the app showed File › Open's panel, with no way to make a new graph.
- Cause: the app declared its document type with the role Viewer. AppKit makes a new document only of a type the app can edit, so `newDocument` did nothing at all: File › New and its key were dead, in every build since the Mac app became document-based. No test or script noticed, because every scripted launch opens a file, and nobody had launched it by hand without one. The controller first told the maintainer that New worked once the panel was cancelled, from reading the code; that was wrong.
- Fix: the document type's role is Editor (`native/apple/PlumCraftMac/project.yml`). Asked whether a launch should open an untitled graph or the panel should gain a button, the maintainer chose "1": a launch with nothing to open, and a Dock click with no graph open, make a new untitled graph (`AppMain.swift`).
- Verified: a scratch launch with nothing to open reported no window before the role change and one ready, untitled, unedited graph after it; the Mac conformance suite 53 of 53 with the person's Library unchanged. Not verified by a person, and no automated check launches without a file yet.
- Fixed: 2026-11-21
### B49. `test_fast_never_session_too_big_and_setup_bound_results_are_not_persisted` fails on the Linux test host while it is busy

- Reported: 2026-11-21
- Report: found by the Linux gate for the egui parity branch; recorded under the standing "record bugs"
- Context: `python/tests/test_persist.py`. It failed in the gate's full Python run (1 failed, 223 passed) and in 4 of 5 runs of the file alone straight afterwards, while the host's load average was above 20; it then passed in 11 runs of the file and one full run (224 passed) once the load fell. The branch changed no Python. The assertion was not captured: the gate keeps only the last two lines of pytest's output.
- Seen again: 2026-11-21, in the gate for the private-strings branch and then in 6 of 6 runs of the file alone, with the host's load average near 19. The assertion, at `tests/test_persist.py:100`: `assert twice(r, {"kind": "t.slow", "values": {"n": 4, "ms": 60, "big": 8_000_000}}) == ("done", 2), "too big"` got `('cached', 1)`: a result meant to be too big to persist was persisted and came back cached. So it is not a timing miss in the test's own waits; whatever decides "too big" gives a different answer while the host is busy. Not looked into further.
- Cause: the test, not the executor. Whether a result is too big is weighed against the node's own work as measured on the wall clock (spec 14.5: its size at ~200 MB/s, twice, under the work time), and the case gave that rule 22 ms of room: about 8.2 MB of outputs costs 81.8 ms to write and read back, and the node sleeps 60 ms and then makes an 8 MB array with `np.full`. numpy asks for huge pages for so large an array (`madvise`, which the host's `transparent_hugepage/defrag` setting honours), so when the host's memory is fragmented (swap full, almost no free blocks of 2 MB, as the gate's builds leave it) the kernel compacts memory while the array's pages fault in. That took 33 to 157 ms on the host, against 0.3 to 0.7 ms otherwise; with numpy's huge-page advice turned off it never took over 7 ms and caused no compaction stalls (`/proc/vmstat compact_stall`), with it on 6 to 9 stalls per first allocation (`claude-debug/b49/probe_thp.py`, `probe_breakdown.py`). The sleep itself was always 60.1 ms. So the work measured was 62.4 to 71.5 ms in passing runs and 87.6, 90.9, 96.8 and 122.8 ms in failing ones (`probe_work_ms.py`), over the 81.8 ms limit, and the rule rightly persisted it. The load average is a proxy: the test failed 2 of 3 runs with the load at 1.6 while memory was still fragmented, and passed 10 of 10 later at a load of 25. The executor does what the contract says: a node that really took 97 ms is worth persisting at 8 MB. Its Rust twin (B37) has a wider margin (32 MB, 320 ms) and probably fails the same way; not checked.
- Fixed: 2026-11-21, branch `fix/b49-persist-too-big`: the tests of what isn't persisted run on a virtual clock (`VirtualClock` in `python/tests/test_persist.py`, the executor's `time` module patched for the test; since `fix/manifest-warn-python-clock`, given to the engine with `Engine.with_clock`, as in Rust), on which a node's work is exactly the time it sleeps, so the fast, setup and too-big cases no longer depend on how fast the machine is. A new test, `test_too_big_is_weighed_against_the_work_measured`, pins the work: 8.2 MB after 60 ms of work is not persisted, after 97 ms (a failing run's value) it is. No SDK code changed. Verified on the Linux test host: with a pytest plugin that adds 40 ms to every `np.full` of 1 MB or more (`claude-debug/b49/b49_stall.py`), the old file failed 3 of 3 and the new one passed 3 of 3, with the host quiet; under a load of 32 niced CPU spinners and 6 processes churning 64 MB to 1 GB arrays (`claude-debug/b49/load.sh`, load average 35 to 53), the old file failed 6 of 10 at the same assertion and the new one passed 30 of 30, and the full Python suite passed 225 of 225; quiet, the old file passed 10 of 10, the new one 30 of 30 and the full suite 225 of 225. `ruff check` clean.
- Issue: none

### B50. A scripted launch of the Mac app writes to the person's real defaults domain

- Reported: 2026-11-21
- Report: found by the before and after check of the decisions branch (`native/decisions-1121`); recorded under the standing "record bugs"
- Context: the Mac app's conformance runs (`--scratch-state`, scratch store, prefs, caches and logs) left `~/Library/Preferences/com.swapwizard.plummer.mac.plist`, the new id's real defaults file, created at the time of the runs; it holds an empty dictionary. The old id's file, `com.plumcraft.mac.plist`, holds keys such as `NSWindow Frame PlumCraft Pop-Out count.count`, the pop-out windows' remembered frames (`Popouts.swift`'s `setFrameAutosaveName`; the Settings window does the same), which look like they came from earlier scripted runs. So `--scratch-state` keeps the core's files in scratch but not AppKit's defaults. Not fixed: a scripted launch could skip frame autosave names, or run in a defaults suite of its own. Not checked: which call created the empty file.
- Cause: the pop-out and Settings windows already skipped their frame autosave names under `--scratch-state`, and saved state was already off (`ApplePersistenceIgnoreState` and `NSQuitAlwaysKeepsWindows` in the argument domain), but every document a scripted launch opened went into the person's recent documents: `NSDocumentController` notes it in the system's shared file list (`~/Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments/com.swapwizard.plummer.mac.sfl4`), which held only the scratch paths of conformance runs (`plumcraft-mac-conformance.*/empty.json`). With AppKit's own recent count, the first launch of each new build also rewrote that list's properties (the code requirement it carries). The empty defaults file was not reproduced: none of the runs below wrote the domain, so which call created it is still not known. AppKit's own panels (the colour panel, Open and Save) keep frames and the last folder in the domain (the old id's file holds such keys), and the domain cannot be pointed elsewhere: `CFFIXED_USER_HOME` and `__CFPREFERENCES_AVOID_DAEMON` both still read the real one. No conformance case opens those panels.
- Fixed: 2026-11-21, branch `fix/b50-scripted-defaults`: the Mac app's document controller (`PlummerDocumentController`, `native/apple/PlumCraftMac/App/AppMain.swift`, made in `main` so it is the shared one) neither notes a recent document nor keeps any (a count of 0) under `--scratch-state`, so AppKit leaves the list alone; a person's launch is AppKit's as before (frames remembered, recents noted). The iPad and iPhone app keep everything in their own container, and a scripted launch there already skips the first-launch flag and scene restoration; not checked on a device. Verified by `native/apple/PlumCraftMac/library-check.sh`, which `conformance.sh` now runs around the app: it snapshots the defaults domain (cfprefsd's answer and the plist's size, time and hash, ByHost too), the saved state (and anything AppKit leaves in its temporary copy), the recent documents list and the app's caches, logs and application support folders before the launch and 10 s after the quit (`SETTLE`; a noted recent landed 3 to 8 s after the quit), and fails the run on any difference. Before the fix the read half failed it (the recent documents list changed); after it, the read half (31 of 31) and the edit half (21 of 21) leave the Library unchanged, as do two launches of new builds. Defect caught on the way: a first try also returned an empty `recentDocumentURLs`, and AppKit wrote that empty list over the person's (which held only the two scratch entries). The empty `com.swapwizard.plummer.mac.plist` is left as it was (42 bytes, 2026-11-21 16:29).
- Issue: none
### B51. The Windows app dropped every mouse click and mouse drag on the canvas

- Reported: 2026-11-21
- Report: found by N5 slice 3's input probe (`native/n5-editing`); recorded under the standing "record bugs"
- Context: slice 2's report said a mouse click injected on the Windows test machine "did not arrive" after the machine restarted, while hover did. With the injector now logging what it can see (its session and integrity level and the app's, both session 1 at High; the foreground window; `GetAsyncKeyState` after each button), the presses and releases did arrive: the app's own status listed `pressed Mouse … L` and `released Mouse …`, and nothing followed. Touch and pen worked.
- Cause: `CanvasView.OnReleased` released the pointer capture before handling the release, and `ReleasePointerCapture` raises `PointerCaptureLost` at once, whose handler took it as a cancel and cleared the press; the release that followed found no press. Every mouse click and drag on the canvas did nothing (right clicks, through `ContextRequested`, still worked).
- Fixed: 2026-11-21, branch `native/n5-editing`: the release is handled first and the capture let go after, and a capture loss the view asked for (`releasing`) is not a cancel. Verified on the test machine with real input through `SendInput`: a click on empty canvas ran `selection.clear`, a drag of a face `edit.move` ("Move Node"), a drag from a pin `wire.connect`, a double click on empty canvas the palette, a click on a row the field editor (`claude-debug/native-n5/r3-probe-run2.txt` and later). Before the fix the same input ran nothing (`r3-probe-run1.txt`).

### B45. In the egui editor, hiding a row from a node's face shifts the rows, and an edit on the face changes the wrong port

- Reported: 2026-11-21
- Report: found by the egui audit the maintainer asked for ("Do the egui audit on [the Linux test host]"); recorded under the standing "record bugs"
- Context: the dev branch at `057ed2d`, on the Linux test host through the kittest harness. Add "Every widget", then `node.face {port: "count", shown: false}`: the labels, fields and pins no longer line up, and dragging the field beside "amount" changed `count` from 3 to 4. The same with Replace's `find` hidden. Graphs saved by the Apple apps can carry this state (Show on Face). Steps and pictures: `claude-debug/egui-audit.md`, defect D1. The audit's other findings in egui (D3 to D8: the palette not offering kinds of engines not started, viewers not following the core's viewer state, no rows reported to the core, no Match the System, double clicks not following the core's rule, a repeated word in the rename window's title) are gaps listed there, not recorded here one by one.
- Cause: egui built each face's rows from `input_rows` less the fields its choice tucks away, while the core's scene (which egui draws labels and pins from) also leaves out the rows `node.face` hides (`rows::shown_rows`). With a row hidden, egui's pin `i` took the scene's row `i` for its label and the catalogue's row `i` for its field, so every row after the hidden one was off by one.
- Fixed: 2026-11-21, branch `native/egui-parity`: egui builds its rows with `shown_rows` (`crates/ui/src/canvas.rs`), so pin `i` is the scene's row `i`; a row whose key isn't the scene's for that pin draws no field (no edit by another port's label). Verified by `crates/ui/tests/kittest_face_rows.rs`: a drag beside "amount" with `count` hidden on Every widget changes `amount` and not `count`; a check box beside "flag" changes `flag`; typing beside "with" with `find` hidden on Replace changes `with` and not `find`; wires into rows before and after the hidden one land on their ports; a graph opened with two rows hidden draws and edits right; and Show on Face in egui's parameters window hides and shows a row. Each failed before the fix (the rows check) and passes after. Pictures: `claude-debug/egui-parity/shots/{before,after}/b45-*.png`.
### B48. `build-xcframework.sh` ignored `CARGO_TARGET_DIR`

- Reported: 2026-11-21
- Report: found while preparing the Apple builds
- Context: the script read the libraries back from the checkout's own `target/` while cargo, honouring `CARGO_TARGET_DIR`, built them elsewhere; in a git worktree pointed at the main checkout's target directory it would have packaged stale libraries, or failed, rather than the ones just built.
- Fixed: 2026-11-21, Apple prep: the script reads from `CARGO_TARGET_DIR` when it is set. Verified by building the xcframework from a worktree into the main checkout's target directory.

### B47. A Mac app built for distribution would carry no engine programs

- Reported: 2026-11-21
- Report: found while preparing the Apple builds (the maintainer's standing instruction to record bugs found on the way)
- Context: the Mac app's Release bundle held only `PlumCraft`. The core looks for engine programs beside the app's own (`Contents/MacOS`), or in the cargo build a test runs from, so on another Mac the audio engine (never in process) and every engine hosted as a process (Engine › Rust · process) would have failed with "no engine binary". In-process text and array, which the suite uses, worked, so nothing caught it.
- Fixed: 2026-11-21, Apple prep (branch `native/n4-apple-prep`): `native/apple/release.sh mac` builds the text, array, audio and craft engines for both Macs and the Mac target's Bundle engines phase puts them in `Contents/MacOS`, each signed with the hardened runtime before the app seals them. Verified on the exported Developer ID build: the text engine switched to a process, audio and array added as processes, a graph run, the three children running from the bundle with the runtime flag. A plain Xcode build still has none (it runs engines from the cargo build, as before).

### B44. The iPad UI tests fail on a second run in the same app container

- Reported: 2026-11-21
- Report: found while getting the device gate ready (the maintainer's standing instruction to record bugs found in gate runs)
- Context: `testInlineEdits` and `testWindowsAndNames` timed out ("its undo name", "count set while hidden") whenever the app had run them before without being reinstalled, reading the status from the file or from the new status element alike; on a device, where the container stays, they would fail every time after the first.
- Cause: every document the iPad app opens autosaves (N4 slice 4), and a scripted launch's `--open-sample` opened the copy in Documents/Samples, so a test's edits were saved into the sample the next run opened, and into the person's samples.
- Fixed: 2026-11-21, the N4 device gate: with `--scratch-state`, `--open-sample` opens a fresh copy of the bundled sample in scratch (`Samples.scratchCopy`). Verified in the iPad simulator by running the five UI tests three times in one container (file, element, file). Still for the device: `device.sh ui`.

### B42. Exporting a value fails on a physical iPad: the export folder can't be written

- Reported: 2026-11-21
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: the conformance suite on the maintainer's iPad (debug build of the dev branch at `9b90500`), case "an array popped out by a person is a picture, and its numbers a grid paged by the core", step 13: `value.export {popout}` answers `<app container>/.cache/plumcraft/open/export/<pid>-<n>: Operation not permitted (os error 1)`. The core writes exports under `.cache` at the container's root, which an iOS app may not write; the simulator allows it, so the case passes there. Share, Save As and Drag Out use the same file, so they are expected to fail on the device too (not tried by hand).
- Issue: none
- Cause: the C ABI core never took the shell's `dirs.caches` (nor `dirs.scratch`): copies for apps and exported values went under the session's default, `HOME/.cache/plumcraft/open` on iOS (the desktop rule, `XDG_CACHE_HOME` or `~/.cache`, since iOS isn't `macos` to `cfg!`). On iOS `HOME` is the app's container, whose root a device app may not write; the simulator allows it, so every test there passed.
- Fixed: 2026-11-21, the N4 device gate (branch `native/n4-device-gate`): the core takes `dirs.caches` and `dirs.scratch` from the config (`plumcraft_ffi::core::Config::{caches, scratch}`): copies and exports go to `caches/open`, autosave's scratch files to `scratch/autosave`; the defaults that derive from `HOME` (open copies, the craft root, the HTTP host's CAS, preferences) use `Library/…` on iOS as on macOS. Found on the way: a short text (carried in its excerpt, with neither `value` nor `full`) exported and copied as nothing; fixed. Tests: `plumcraft-ffi::dirs` runs itself again with `HOME` read-only and every `dirs` root writable and takes values out every way the apps do (export, Share, the graph's Share, autosave in scratch and in the document's file, the open-value cache's root); it fails without the fix ("copies for apps go to …/home/Library/Caches/plumcraft/open"). In the iPad simulator with its data container's root made read-only (`chmod 555`, as a device has it): slice 6's build failed the same step as the iPad ("…/.cache/plumcraft/open/export…"), this branch's passed the whole suite, 52 of 52 (`claude-debug/native-n4/s7-b42-sim.sh`). Save As goes through the engine to the path the shell's ask gave (Documents) and the compiled-in engines save no formats, so it was not taken. Still for the device: the suite's export step on the iPad.

### B43. The graph's Parameters window is not reported open on a physical iPad

- Reported: 2026-11-21
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: the same run, case "the Cache Report and the graph's Parameters window are the core's: opened, read, edited and closed", step 11: `scene.get` has no `view.windows.params` where the case wants `true`. Passes on the Mac and in both simulators. One run only; whether it is timing on the slower debug build or a real difference is not looked into.
- Issue: none
- Cause: the iPad shell's, not the core's or the case's. Sheets the core opens and closes arrive faster than UIKit's transitions run (a script, the suite, a slow device). The Messages window (the case before this one in the suite) was dismissed while its presentation was still running, and the Cache Report was then presented over it; when the Messages sheet went it took the Cache Report and the Parameters sheet over that with it, and the shell, seeing them gone, told the core both were closed. Reproduced in the iPad simulator, launched as `device.sh` launches the app (Debug, `--scratch-state`): Messages opened and closed, then the Cache Report and Parameters opened at once, Parameters closed by itself within 0.11 s in 6 of 20 rounds (`claude-debug/native-n4/s7-b43-probe.py --messages`); the case alone looped 30 times never failed, which is why it passed everywhere but the device.
- Fixed: 2026-11-21, the N4 device gate: every presentation change the core drives on the iPad (core windows, Messages, Settings, a value's sheet replaced or closed) goes through one queue, `Presentations.whenSettled`, one at a time, in order, once nothing presented is mid-transition; a core window counts as closed by something else only after its own presentation has run. After it: 0 of 20 rounds of the probe, the case 30 of 30. New conformance case "windows opened as another closes stay open: the shell never closes what the core opened" with a new step form, `holds` (a condition that must stay so `for` a time, in both runners); it did not fail on slice 6's build in its one run there, so it catches the defect only some of the time. Still for the device: the suite on the iPad.

### B41. A run setting kept in the preferences is sometimes lost on the Linux test host

- Reported: 2026-11-20
- Report: "record bugs" (the maintainer's standing instruction for failures found in gate runs)
- Context: `plumcraft-ffi::settings_autosave` `settings_are_the_cores_kept_in_its_prefs_and_every_document_follows_them`, `crates/ffi/tests/settings_autosave.rs:84`, added in N4 slice 4. After `theme.setToken`, `view.animationSpeed {ms: 1000}` and `lifecycle.appearance {dark: true}`, `settings.get {fromDisk: true}` gives Animation speed 500 (the default), not 1000. On the Linux test host it failed 1 of 10 alone at the dev branch before slice 5 and 6 of 10 at slice 5, which did not touch the settings code; it has not failed on the Mac. It may be the test's timing or a setting really lost when a document follows the shared look (`settings::follow` sets the animation speed from the preferences without a guard, unlike the run colours).
- Issue: none
- Cause: a defect in the editor, not in the test. `settings::after_command` wrote all three run settings (run colours, animation speed, Pick Engines Automatically) from whichever document ran a run command. Every document runs `engine.auto` when its first engine connects; a second document that connected after the first had set the speed, but before it had followed the shared look, wrote its own stale value back. A person with two windows could lose the setting the same way.
- Fixed: 2026-11-21, N4 slice 6: only the setting the command changed is kept (`Look::keep_run_pref`), and `settings.get {fromDisk}` reports the file's run settings. Test: `settings::tests::a_run_setting_changed_in_one_document_survives_another_that_has_not_followed_yet`. On the Linux test host the C ABI test file passed 100 of 100 runs at `e8a051b` (6 of 10 failed before).

### B36. A short clip's spectrogram is computed on the editor's thread

- Reported: 2026-11-20
- Report: "record bugs" (found while fixing B32)
- Context: `AudioView::sync` computes the spectrogram in place for clips under `SPEC_SYNC_SAMPLES`. A frame that shows a watched audio value took about 80 ms alone and 74 to 319 ms under load on the Linux test host (median 122 ms). It is not a wait for an engine, so B32's test no longer flags it.
- Issue: none
- Fixed: 2026-11-20, PR #131 (N4 slice 5): the spectrogram is computed on the media workers.

### B34. `a_save_in_the_app_feeds_the_nodes_downstream_and_runs` fails under the Linux test host's full parallel run

- Reported: 2026-11-20
- Report: "record the others"
- Context: `plumcraft-session` `doc_cmds::tests`: a poll returned `None` where the test expected the run's result. Failed in the baseline run of the dev branch at `b4a561b`; passes alone.
- Issue: none
- Cause (2026-11-20, confirmed): the test's saves came too close together for the file system's clock. Its last two saves (`{"doc": 1}` and then `{"doc": 4}`) are the same size as the save before each, and come 0.4 to 0.8 ms after it. The copy's clock on the Linux test host ticks once a millisecond. So often neither the size nor the modification time changed, and `poll_doc_edits` never saw the save. That is lines 784 and 791. An instrumented run printed an unchanged stamp before every failure. It is not load: on that host the test failed 91 of 100 runs alone, one at a time, and 123 of 200 with 40 copies at once. It passes on the Mac. That APFS's finer timestamps are the reason is inferred; it was not re-run there. Like B13, this is a test that met a clock. The session drops nothing in real use: a copy holds still for one 500 ms poll before it is read, so a later save lands at least 500 ms after the change the session last saw. Only a file system whose clock ticks more slowly than that (FAT, 2 s) could hide one. The test's polls run on a made-up clock, well under a millisecond apart.
- Fix: the test saves through a helper that writes the copy and, while its size and time are unchanged, waits a millisecond and writes it again, for at most 5 s. `doc_edits.rs` now states the limit. After: 0 of 100 alone and 0 of 200 with 40 copies at once.
- Fixed: 2026-11-20, PR #120

### B33. `hostile_files_never_panic_a_decoder` fails under the Linux test host's full parallel run

- Reported: 2026-11-20
- Report: "record the others"
- Context: `plumcraft-media::hostile`, `crates/media/tests/hostile.rs:98`: a decode went past the test's 5 s limit. Failed in the baseline run of the dev branch at `b4a561b`; passes alone.
- Issue: none
- Cause (2026-11-20, confirmed): the test, not a decoder. One input is slow. It is the empty SVG that claims `width='1e30'`, drawn by `svg::rasterise(&f, 1e9)`. The clamp holds: it is drawn at 4096 × 4096 (`MAX_RENDER_PIXELS`), not at its claim. Drawing those 16.8 million pixels and making them opaque takes 4.1 s in the test profile, against 0.23 s in release, so a 5 s limit on each file fails under load. Each file was timed alone through each decoder: every other input takes under 1 ms, apart from the first SVG parse, which loads the system fonts once (0.3 s). With 40 copies at once on the Linux test host it failed 37 of 200.
- Fix: the picture decoders stay timed against 5 s, which they reach only by believing a header. Each SVG raster is checked against the clamp that bounds its work (each side at most 4096, at most `MAX_RENDER_PIXELS`, a buffer of exactly w × h × 4) instead of being timed. After: 0 of 200 under the same load.
- Fixed: 2026-11-20, PR #119
### B32. `values_never_wait_for_an_engine` fails under the Linux test host's full parallel run

- Reported: 2026-11-20
- Report: "record the others"
- Context: `plumcraft-ui::engine_thread`, `crates/ui/tests/engine_thread.rs:546`: a spectrogram took longer than the test's 150 ms limit. Failed in the baseline run of the dev branch at `b4a561b` (80 cores, all tests in parallel); passes alone.
- Issue: none
- Cause (2026-11-20, confirmed): the test, not the editor. It held every frame under half of a 300 ms `resolve`, which measured the editor's own work in a frame, not a wait. With 40 copies at once on the Linux test host it failed 34 of 200: 32 in "watch n5" (the audio value; frames of 74 to 319 ms, median 122 ms), whose frame computes the Watched panel's spectrogram in place (`AudioView::sync`, clips under `SPEC_SYNC_SAMPLES`), and one each in the pop-out and compare phases. In all 200 runs no `resolve` was made on the editor's thread. Alone the audio frame is about 80 ms.
- Fix: the test holds each `resolve` at a gate until the editor has drawn three frames with it held, and fails if the gate has to give up on one (10 s): a frame that waited for an engine, directly or through a lock or channel, never returns while the call is held, however loaded the machine. Frame times are printed, not asserted. A deliberate fault (the Watched panel spinning until its value arrives) fails it. After: 0 of 200 under the same load, with audio frames up to 225 ms. The in-place spectrogram is still a stall of the editor's thread; moving it off the thread is a separate change.
- Fixed: 2026-11-20, PR #118
### B35. A blocking read's result could turn pending again (`blocking_reads_in_place_and_shares_the_cache` failed now and then)

- Reported: 2026-11-20
- Report: found by the B30 fix's full run on the Linux test host (failed at `crates/media/src/service.rs:1055`, "then cached"); 1 in 10 when run alone there.
- Cause (2026-11-20, confirmed): `MediaService::blocking` first calls `get`, which queues the request for a worker, then makes the value itself and keeps it as done, with the key still in the queue. A worker that reached the key after that set the done entry back to working and made it again, so a `get` meanwhile answered pending. Nothing else turns a done entry back. Not B31's cause (B31's test read too early); found while reading the same path. Reproduced 1 in 1,200 under load (0 in 300 alone); 0 in 1,200 after.
- Fix: a worker skips a queued request that is no longer queued (`run_one`). `a_queued_request_settled_in_place_is_not_made_again` fails without it (3 reads, want 2).
- Fixed: 2026-11-20, PR #117
- Issue: none

### B31. `a_request_answers_at_once_and_arrives_on_a_worker_with_a_wake` fails now and then on the Linux test host

- Reported: 2026-11-20
- Report: "record the others" (the four tests that fail only under the Linux host's full parallel run)
- Context: `plumcraft-media` `service::tests`. Failed once in the full run for N2 slice 2 (`85695df`) at `crates/media/src/service.rs:909`, "the shell was woken": the picture had arrived and the wake count was still zero. Passed 30 of 30 when run alone on the same host. The test is from N1 slice 6.
- Cause (2026-11-20, confirmed): the test read the wake counter too early. A worker keeps its result under the service's lock, lets the lock go, then calls the wake; the test asks without waiting for the wake, so a worker descheduled between the two lets it see the picture with the count still zero. That order (kept, then woken) is the right one: whoever a wake sends to ask again finds the result. Reproduced 112 times in 1,200 (60 copies at once, 20 rounds, with 40 busy loops alongside). Reading the path also found a real lost wake: when `MediaService::blocking` (`doc.open`, `value.open`) settled a request a worker was already making, the worker found it settled and neither kept its result nor woke, so a shell that had been told "pending" never heard; and a queued request settled that way was made a second time when a worker reached it.
- Fix: the test waits for the wake (10 s limit) and says what order is guaranteed; `blocking` wakes when it settles a request a worker had, after keeping it; a worker skips a queued request already settled. Three new tests: a wake always finds its result (200 rounds, the wake itself asks), and two for the in-place cases, which fail without the fix. After: 0 in 1,200 on the same load.
- Issue: none
- Fixed: 2026-11-20, PR #117
### B30. Video to frames fails with ffmpeg older than 5.1 (Ubuntu 22.04)

- Reported: 2026-11-20
- Report: "Bug:" (in answer to: the array engine's video reader passes `-fps_mode passthrough`, which ffmpeg only has from 5.1; Ubuntu 22.04 packages 4.4.2)
- Context: `video_goes_to_frames_and_back` (`crates/array/tests/channels.rs`) fails on the Linux test host with "video: /usr/bin/ffmpeg failed: Error splitting the argument list: Option not found". The option is passed at `crates/array/src/channels/video.rs:248`. GitHub's runner has a newer ffmpeg, so CI doesn't show it.
- Issue: #113
- Cause (2026-11-20, confirmed): `-fps_mode` was added in ffmpeg 5.1; before that the same setting is `-vsync passthrough`. It is the only option the video nodes pass that 4.4 lacks: on ffmpeg 4.4.2 the reader with `-vsync` (rgb24 and grayf32le), the ffprobe call, and the MP4 (libx264), MOV (prores_ks, profile 3, yuv422p10le) and GIF (palettegen/paletteuse, `-loop 0`, rgb and gray) writers all ran, and each written clip read back.
- Fix: the reader asks `ffmpeg -version` once per ffmpeg program and passes `-vsync` for a version before 5.1, else `-fps_mode`; an answer with no version in it (a git snapshot, or anything unparseable) gets `-fps_mode`. Test: `frames_pass_through_with_the_option_this_ffmpeg_has`.
- Fixed: 2026-11-20, PR #116

### B28. The hash gate fails on `main`'s Linux CI: "2 runtime commonkit v0.2 lines, want 1"

- Reported: 2026-11-19
- Report: "record the two main CI failures as bugs"
- Context: the `gate` job's "hash gates" step (`scripts/check-hash-gates.sh`) fails on `main` (latest `34f6aea`, run https://github.com/seantrue/plumcraft/actions/runs/43405581253) and on the native dev branch's pull requests, printing `hash gate: 2 runtime commonkit v0.2 lines, want 1; other versions: none`. The same script prints `hash gates: ok` on the Mac, on `main`'s code and on the dev branch's.
- Checked since (2026-11-19): on the Mac the script also passes under Rust 1.100.0, and `cargo tree … -i commonkit --target x86_64-unknown-linux-gnu` prints the same lines as for the Mac. So neither the Rust version alone nor the target's dependencies explains it. The runner's own `cargo tree` output has not been captured.
- Issue: #102
- Cause (2026-11-19, confirmed): CI sets `CARGO_TERM_COLOR=always`, so `cargo tree` wraps its `[build-dependencies]` heading in colour codes, and the script's `sed`, which cuts the output at that heading, no longer matches it: both `commonkit` lines are counted. Nothing to do with Linux or the Rust version; `CARGO_TERM_COLOR=always scripts/check-hash-gates.sh` fails the same way on a Mac. Found by making the script print the tree it counted and reading the runner's log.
- Fix: the script sets `CARGO_TERM_COLOR=never`, and prints the tree when the count is wrong.
- Fixed: 2026-11-19, on the native dev branch only, PR #109.

### B29. `mode2_python_kill_9_mid_run_recovers` fails on Linux CI: "the engine is not connected"

- Reported: 2026-11-19 (found by the controller on the native dev branch's pull request #101; not reported by the maintainer)
- Report: `plumcraft-text::conformance mode2_python_kill_9_mid_run_recovers` panicked at `crates/text/tests/conformance.rs:649` with `run: unavailable (-32006): the engine is not connected`, after 0.855 s, on run https://github.com/seantrue/plumcraft/actions/runs/44615077606 and again on its rerun: 2 of 2. Everything else passed (1,342 of 1,343). The Rust engine's twin, `mode2_kill_9_mid_run_recovers`, passed both times.
- Context: the same test passed on the four Linux runs before it that day (#98, #99, and #100 twice). The pull request's changes (N1 slices 4 and 5) touch nothing in `crates/text`, `crates/host`, `crates/conform`, `crates/proto` or `python/`. Not reproduced on the Mac: 40 runs alone and 48 runs twelve at a time all pass.
- Since (2026-11-19): intermittent, not constant. The Linux run of pull request #104 (the same code plus N1 slice 6) passed it, under md5 and b3: 2 failures in 7 Linux runs that day. #101 was merged into the dev branch with it failing (the maintainer's CPM, given to the slice 6 agent).
- Suspect, read from the code and not confirmed: in `crates/host/src/process.rs` the reader thread's end fails the run in flight before the supervisor has set the state to `EngineDown` (it reaps the process first, holding the `child` lock). `plumcraft_conform::kill_9_mid_run_recovers` gets the failed reply, then calls `wait_ready`, which returns at once on the stale `Connected`; `pid()` waits for the reap and is then not the first pid; the next `run` finds no writer.
- Open question: fix the host (state goes down before replies fail) or make the check wait for a new connection? The first changes what an editor sees after a crash, for the better if the suspicion is right.
- Issue: #103
- Cause (2026-11-19, confirmed; the suspect above had the order wrong): in `crates/host/src/process.rs` the supervisor failed the pending calls and live runs first and set the state to `EngineDown` after. Between the two, the check got its failed reply, `wait_ready` returned at once on the dead connection's `Connected`, `pid()` was `None` (already reaped, so "not the first pid" held), and the next `run` found no writer. A unit test that reads the state on hearing of the failure fails every time with that order.
- Fix: the state goes down first (`Inner::went_down`). An editor now sees the engine-state event before the failed run's reply, not after.
- Fixed: 2026-11-19, on the native dev branch only, PR pending. Not shown on Linux beyond CI passing: the failure was 2 runs in 9.

### B27. Eight editor kittests fail on `main`'s Linux CI, as in closed issue #89

- Reported: 2026-11-19
- Report: "record the two main CI failures as bugs"
- Context: found while comparing the native dev branch's pull requests (#98, #99) with `main`. The `gate` job's "cargo nextest, under md5 and then b3" step fails on every recent run of `main` (latest `34f6aea`, run https://github.com/seantrue/plumcraft/actions/runs/43405581253) with the same eight tests of `apps/plumcraft-editor/src/lib.rs`. `tests::audio_kittest`: `a_1khz_tone_peaks_in_its_spectrogram_bin`, `play_without_an_output_says_so_and_does_not_panic`, `the_audio_demo_runs_with_waveforms_and_an_audio_view`. `tests::engine_kittest`: `a_pick_adds_or_changes_an_engine_and_clear_starts_over`, `a_switch_cancels_the_run_in_flight`, `attach_connects_to_a_running_http_engine`, `switching_text_engines_runs_after_every_switch`, `text_to_audio_keeps_the_graph_and_reports_its_problems`. They pass on the Mac. Issue #89 ("CI gate: 8 editor kittests can't find the engine binaries (no engine binary at …/out/plumcraft-text-engine)") was closed as completed on 2026-11-12; the same eight still fail. The panic messages of the latest run have not been read against #89's, so that it is the same failure and not only the same tests is not confirmed. On the native dev branch the lookup (`engine_binary`) has moved to `crates/editor-core/src/engines.rs`.
- Found since (2026-11-19): it is #89's failure. `main`'s run panics with `no engine binary at /home/runner/work/plumcraft/plumcraft/target/debug/build/plumcraft-editor/62bf2a2969a8c725/out/plumcraft-text-engine` (and `…/plumcraft-audio-engine`). The test program on CI is at `target/debug/build/<package>/<hash>/out/`, not `target/debug/deps/`; `engine_binary` looks beside the program and, when the program is in `deps`, one folder up, so it finds nothing. CI installs the newest stable Rust, 1.100.0 (2026-11-09) on these runs; the Mac has cargo 1.99.0 and builds test programs in `target/debug/deps/`. That 1.100 moved them is inferred from those two facts, not read from its release notes.
- Also hit by: `plumcraft-ffi::switch starting_a_process_engine_never_holds_the_core_thread`, new in the native dev branch's pull request #100, which fails on Linux CI with the same message (`…/build/plumcraft-ffi/9d267b73553d9f8b/out/plumcraft-text-engine`).
- Issue: #89 (reopened 2026-11-19)
- Cause: confirmed with a scratch project built under both versions (`claude-debug/b27-layout/`): cargo 1.99 builds a test program in `<profile>/deps/`, cargo 1.100 in `<profile>/build/<package>/<hash>/out/`; the workspace's binaries are in `<profile>/` under both. Reproduced on the Mac with `cargo +1.100.0 test -p plumcraft-ffi --test switch`.
- Fix: `plumcraft_editor_core::engines::profile_dir` knows both layouts; `engine_binary` and the two test helpers that build a missing engine (`apps/plumcraft-editor/src/lib.rs`, `crates/ffi/tests/switch.rs`) use it. Test: `a_test_program_finds_engine_binaries_in_either_cargo_layout`. Under 1.100 on the Mac the switch test and the editor's kittests fail before and pass after; on Linux CI the pull request's run had no failing test.
- Fixed: 2026-11-19 on the dev branch `feat/native-n0` only, PR #100 (`b2bd46c`). `main` has its own copy of the lookup in `apps/plumcraft-editor/src/lib.rs` and still fails there; #89 stays open until the dev branch reaches `main`.

### B26. "Wrap seamlessly at the edges" on Simplex Noise's `tileable`

- Reported: 2026-11-11 (found while checking the kernel library's #65 against this port; filed as #84)
- Report: the `tileable` tooltip says "Wrap seamlessly at the edges", but simplex wraps its lattice in skewed coordinates, so the output repeats along the diagonal and its edges don't tile on x or y, on the kernel library's backends or here. Measured seams are the same size as with `tileable` off.
- Context: the kernel library made the same correction to its docs (its `simplex2d` docstring: "The output is not seamless along x or y"); real seamless tiling is the kernel library's #83 (a feature request), which would change this port too. impressionist's copy of the tooltip (its `docs/BUGS.md` B2) gets the same wording in its own branch.
- Issue: #84
- Cause: the tooltip described what tiling was meant to do, not what the kernel does.
- Fix: the tooltip (`crates/array/src/generate/mod.rs`) is now "Wrap the noise lattice; not seamless on x or y (smooth only)". Test: `simplex_tileable_tooltip_does_not_promise_seamless_edges` (`crates/array/tests/catalogue.rs`), which fails on the old text.
- Fixed: 2026-11-12, branch `fix/83-84-simplex-tile`

### B25. Simplex Noise swapped the tile periods on non-square tileable noise

- Reported: 2026-11-11 (found while checking the kernel library's #65 against this port; filed as #83)
- Report: `simplex2d` puts pixel (r, c) at (r / h, c / w) × frequency, but passed `tile_w = w / frequency` as the period of the row-based coordinate and `tile_h = h / frequency` as the column-based one's. The kernel library wraps the row-based coordinate with rows / frequency (its kernels' `width` is the rows). Square sizes, and `tileable` off, were unaffected; non-square tileable output differed from the kernel library by up to 0.72 (24 × 16, measured). Every `simplex.json` case was square, so none caught it.
- Context: the doc comment also said the numba kernel "is only right for" square sizes, which went stale when the kernel library's #75 (`29f8403`) fixed numba's loop.
- Issue: #83
- Cause: the port took the kernel library's `tile_w`/`tile_h` names as columns/rows, where the kernel library means rows/columns.
- Fix: `crates/array/src/generate/noise.rs`: the row-based coordinate wraps every `h / frequency` cells and the column-based one every `w / frequency` (`tile_rows`, `tile_cols`; `simplex2d_single`'s parameters renamed `tile_x`, `tile_y`). The doc comment now says what the kernels compute. `crates/array/tests/fixtures/generate/simplex.json` gains two non-square tileable cases (24 × 16 and 12 × 30) recorded from the kernel library's `main` (`6fa5e01`, numba backend) with the plum coordinator's probe `claude-debug/pc8384_simplex_fixture.py`, which first checked that the five existing cases still reproduce exactly; the existing cases are byte-identical. `simplex_matches_numba` reads `height` and `width` when a case has them and checks the shape. It fails on the old port (max error 0.72 at 24 × 16) and passes exactly (0) on the new.
- Fixed: 2026-11-12, branch `fix/83-84-simplex-tile`

### B24. xyY of black is NaN

- Reported: 2026-11-11 (found while porting colormath; filed upstream as the kernel library's #66)
- Report: the colour spaces gave x = y = NaN for black in xyY, and NaN RGB back, because colormath's OpenCL `xyz2xyY` divided 0 by 0, while its numba port gave 0.
- Context: the kernel library's #66 (now closed) settled it: an exact zero sum X+Y+Z has x = y = 0, and Y is kept. The maintainer chose that option "With check" (an explicit `sum != 0` test), and "Patch the 10 values directly" for this repository's fixture.
- Issue: the kernel library's #66 (closed)
- Cause: the ports copied the kernel's `x / sum`, `y / sum` with no zero check.
- Fix: `if sum != 0 { x/sum, y/sum } else { 0, 0 }`, Y kept, in the three ports: `crates/engine/src/color.rs` (`xyz_to_xyy`), `crates/array/src/colour/mod.rs` (`rgb_to`) and `python/src/plumcraft_engine/color.py` (`_xyz_to_xyy`, with `np.where`). Bit-identical to the kernel library's numbax `xyz2xyY` (jit and numpy fallback) on 100,000 random rows plus black, (0.5, -0.5, 0), -0 and (0.25, 0.25, -0.5). The inverses already gave XYZ 0 at y = 0 and are unchanged. `crates/engine/tests/fixtures/color.json`: the 10 nulls became 0.0, with nothing else changed (a JSON-aware comparison): `spaces.xyY.from_rgb` planes 0 and 1 (x, y) and `spaces.xyY.to_rgb` planes 0, 1 and 2 (r, g, b), each at samples 0 and 41. Tests: the fixture comparisons in `crates/engine/tests/color.rs` and `python/tests/test_color.py` (xyY) fail with the old ports and the patched fixture; the new `xyy_of_black_is_zero` (array) fails with the old port.
- Fixed: 2026-11-11, branch `fix/66-xyY-black`

### B23. A run marks every node queued, not only the ones it computes

- Reported: 2026-11-11 (found by the AI while evaluating run animation; the maintainer chose to fix it first)
- Report: "Go with proposed colors." (approving the run-animation proposal, which listed this as a fix to make first)
- Context: `RunState::request` (`crates/session/src/run.rs`) marked every node of the graph `Queued`, while the engines compute only what the request needs (`docs/scheduler.md` section 1). Clicking a node's title runs that node and what feeds it, yet every node, downstream ones included, showed "queued" until the reply, which then cleared their state, so a node outside the run lost its earlier "done" or "cached". The run's progress counted the whole graph too.
- Issue: none
- Cause: as above; the session never worked out which nodes a request needs.
- Fix: `run::needed` mirrors the scheduler's rule (the targets, or the sinks, plus watched nodes, and their ancestors; a wire into a bound input is dropped). Only those nodes are queued and counted, and the rest keep what they showed. Tests: `run::queued_tests` (four). Not checked in the real editor.
- Fixed: 2026-11-11, PR #81

### B22. Node thumbnails aren't in step with the latest run

- Reported: 2026-11-11 (from an application built on PlumCraft 0.6.1)
- Report: "node thumbnails are not synchronize"
- Context (the maintainer): a node's thumbnail lags behind that node's latest run, and thumbnails from different runs are shown side by side.
- Issue: #70
- Cause: three things in the session's run state (`crates/session/src/run.rs`). (1) A node's thumbnail was replaced only when a run sent a new one: a node done or cached in a run that made no thumbnail (an output with none, a picture that couldn't be read) kept an earlier run's picture under its new result. (2) While a run computed the graph again, each node kept its last thumbnail until it got there, by design so previews don't blink, but nothing marked it as the earlier run's: finished and waiting nodes looked alike. (3) An edit's stale mark went only at the run's reply, so a node this run had already finished stayed stale and its new thumbnail faded like the old ones until every node was done.
- Fix: the reply's thumbnail, or this run's preview, is the node's thumbnail, and none when the run made none. `NodeRun::thumb_generation` says which run a thumbnail came from, and `thumb_is_older` is true while a newer run is computing the node and hasn't sent one. The canvas fades such a thumbnail, excerpt or waveform, with a hover saying it is from the previous run (`canvas::thumb_faded`). A node stops being stale the moment the run made after the edit finishes it. Tests: `run::thumb_tests` (four, two fail without the fix) and `kittest_run_status.rs` (one). Checked in the real editor (`claude-debug/run_status_probe.py`, `claude-debug/b22/4-mid-run.png`): mid-run, the first node is up to date with the new text while the nodes waiting for the slow one keep their earlier excerpts, faded.
- Not fixed here: `stale_since` keeps the first edit that made a node stale. A node already stale when a run is requested and edited again during that run is taken as up to date when the run finishes it. This was already so at the reply, and now it happens when the node finishes.
- Fixed: 2026-11-11, PR #80

### B20. Can't tell which nodes are running

- Reported: 2026-11-11 (from an application built on PlumCraft 0.6.1)
- Report: "can't tell which nodes are actually running"
- Context: the node header shows a spinner while a node runs (`crates/ui/src/canvas.rs`, the run-status drawing).
- Issue: #68
- Cause: a node's stale mark outranked its run state. An edit makes the node and everything downstream stale, and the run that follows (every auto-run) computes them again, but the mark only went at the run's reply. Meanwhile each of them kept the stale border and the word "stale", queued and running alike; running showed only as the 12-point spinner in place of the state dot, and as a progress bar for a node that reports progress. Seen in the real editor (`claude-debug/b21/4-mid-run.png`): the running node looked like the queued ones.
- Fix: while a run computes a node (queued or running) its run state shows, not stale: the border in the queued or running colour (running a little wider), and the header says "queued", or "running" in the running colour with the percentage when the node reports one. `run_view::shows_stale` decides the stale label and border. A run that never reaches the node (cancelled, the engine gone) leaves it stale again. Tests: `kittest_run_status.rs` (two). Checked in the real editor (`claude-debug/run_status_probe.py`, `claude-debug/b20/4-mid-run.png`).
- Fixed: 2026-11-11, PR #79

### B21. A node that has never completed a run shows "stale"

- Reported: 2026-11-11 (from an application built on PlumCraft 0.6.1)
- Report: "stale is not a good status for node that has never completed a run"
- Context: the node header's run status (`crates/ui/src/canvas.rs`, the stale label and border).
- Issue: #69
- Design (the maintainer): a node with no completed run shows **not run**; stale stays for a node whose last result was invalidated by an edit.
- Cause: staleness was tracked for every node with a run record, whatever that run did. A node that failed, was skipped or cancelled, or that a run never reached, went "stale" on the next edit upstream, though it had no result to be out of date.
- Fix: `NodeRun::completed` remembers that a run has completed the node (done or cached), and `NodeRun::is_not_run` is true while none has and none is computing it, for a node never in a run, one left out of a run, or a failed, skipped or cancelled one after an edit. The header says "not run" for those, and the stale label and border are kept for a completed node. A failed, skipped or cancelled node still says so until an edit. `ui.inspect` gives `notRun` per node. Tests: `run::not_run_tests` (session) and `kittest_run_status.rs`. Checked in the real editor (`claude-debug/run_status_probe.py`, pictures in `claude-debug/b21/`): after an edit, the demo's failing node and the one after it say "not run", the rest "stale".
- Fixed: 2026-11-11, PR #78

### B10. Hashing makes content-addressed storage slow for large arrays

- Reported: 2026-11-05
- Report: "Fix the slow hashing issues for content addressed storage."
- Context: `ROADMAP.md` "Next: BLAKE3 handles for large local arrays (D3's measurement)". On an M4 Max with 0.2.1, a 12 MP float32 RGB array (144 MB) crossing between engines through the coordinator adds about 550–600 ms (36 MB: 130–190 ms), more than the nodes on either side; MD5 of 144 MB took 190 ms against 10 ms for BLAKE3 (`b3sum`, multithreaded). The rest of a crossing isn't profiled yet. Code: `Store::publish_array` and its read path (`crates/store`), `crossing_inputs` (`crates/coordinator/src/runner.rs`); the BLAKE3 key helper would go in commonkit (`../pycommonkit`), which D3 held back until a measurement favoured it.
- Issue: #28
- Design (the maintainer, 2026-11-10, of three offered): "Go to BLAKE3 now", then "Yes, publish commonkit".
- Cause: measured on this Mac with a release build (`claude-debug/b10-hash-timing`), 144 MB `<f4`: an array's publish hashed it twice (the engine's handle, then `Store::publish_array` again), about 350 ms of MD5, and a read checked it once more (170 ms): 528 ms, the 550–600 ms reported. BLAKE3 hashes it in 8–12 ms.
- Progress: commonkit 0.1.2 takes 64-digit digests (pycommonkit #17, #18). PlumCraft (`fix/28-blake3-handles`): `b3` handles in proto (shared vectors, checked against Python's `blake3`), the store publishes under the engine's handle without hashing again and checks a handle in its own algorithm, runs ask for `b3` (`RunRequest.algo`) when `hash.b3` was negotiated, and a coordinator clears it when an engine in the run lacks it. 144 MB publish and read back: 528 ms before, 29 ms with `b3` (355 ms with md5 hashed once). The Python SDK followed (`fix/28-blake3-python`): `blake3` as a dependency, `hash.b3` and `data.algos` in its hello, `b3` handles when a run asks, every handle checked in its own algorithm, pyuncommonkit 0.1.2.
- Fixed: 2026-11-10, PRs #74 (Rust) and #75 (Python)

### B18. Port and parameter labels have no translation table

- Reported: 2026-11-06
- Report: "port and parameter labels should have a \"translation\" table, allowing nicer display and formatting. (max_array_width -> Max. width) This table should be both claude AND maintainter editable."
- Context: the node canvas shows a port's name, or the engine's `label` for it, as it comes (`crates/ui/src/canvas.rs`, `port_label`). Raw parameter keys from engines and craft packs (`max_array_width`, `scaleStyles`) show as they are. The table should be a file both the maintainer and Claude can edit.
- Issue: #59
- Design (the maintainer, 2026-11-10, of three offered): "Per pack + user file". Each pack carries its own table, an optional user file overrides it, and core holds only the lookup, no names.
- Fixed: `plumcraft_proto::labels` reads a table (`[labels]`: a port name → label, or a node kind → a table of them; hostile entries skipped, in the hostile-input sweep). Layers, lowest first: the engine's own label; a craft pack's `[labels]` (applied by `plumcraft-craft` to its own nodes) or an application's table (`Session::use_labels`); the user's `labels.toml` beside the preferences. The session keeps the engines' catalogue and relabels it whenever a catalogue or a table changes; `labels.reload` reads the file again and `labels.show` lists what is in effect. PhotoCraft's pack labels `scaleStyles`. Checked in the real editor (`claude-debug/b18_labels_probe.py`): a `labels.toml` written while it ran relabelled the demo's ports on `labels.reload`.
- Not done here: a table for the research software's names (`max_array_width`) belongs to the research software's application, which calls `use_labels` with it.
- Fixed: 2026-11-10, PR #73

### B13. `pdfcraft_runs_a_chain_by_document_id` fails when a save straddles a second

- Reported: 2026-11-05
- Report: "yes, add as B13" (after the test stopped the first `release.sh` run for 0.2.4)
- Context: `crates/craft/tests/pack_pdfcraft.rs:94`, against the installed PdfCraft 0.17.0. The test runs the same chain twice and allows fewer than 16 differing bytes, as PdfCraft's saves depend on the clock in whole seconds through a few bytes of one compressed stream. In `release.sh`'s serial `cargo test` run it failed with "166 of 1498 bytes differ" (`claude-debug/release-0.2.4.log`); alone it passed 3 of 3, and the same commit passed nextest locally and on CI.
- Issue: none
- Seen again: 2026-11-06, in the full `cargo test` gate for #47 (status-bar messages); it passed alone.
- Cause: PdfCraft writes the save time, in whole seconds, as a `D:` date inside a Flate-compressed object stream. A save a second later re-compresses that stream, so hundreds of bytes move and the file can be a byte longer (reproduced with a 1.1 s pause: 487 bytes differ, 1496 against 1497 bytes). The test allowed fewer than 16 differing bytes at equal length.
- Fix: the test compares the two PDFs' streams, inflated, with dates masked and the cross-reference stream left out, and now always pauses past a second so the case it guards is the one it meets. It also checks that the comparison still tells two watermarks apart.
- Fixed: 2026-11-10, PR #71

### B19. A popout doesn't stay on top while the main window is being edited

- Reported: 2026-11-07
- Report: "popout shoud stay on top while main window is being edited"
- Context: reported from a session of the research software (the research software runs PlumCraft's editor); popouts are `crates/ui/src/popouts.rs`. Clicking into the main window to edit puts it in front of the popout.
- Issue: none
- Cause: pop-outs were ordinary windows (`WindowLevel::Normal`), so a click into the main window ordered it in front of them. Reproduced in the real editor (`claude-debug/b19_popout_on_top_probe.py`): after raising and clicking the main window, it was listed in front of the pop-out.
- Fix: while one of the editor's windows (the main one or a pop-out) has focus, every pop-out is `AlwaysOnTop`; when another application takes the focus they go back to `Normal`, so they don't float over other programs (`crates/ui/src/popouts.rs`). `ui.inspect` gives each pop-out's `on_top`. The probe: the pop-out stays in front (the floating level, layer 3) after the main window is clicked, drops to layer 0 with Finder in front, and floats again when the editor returns.
- Fixed: 2026-11-10, PR #72

### B17. Packs aren't versioned or chosen by the application's version

- Reported: 2026-11-06
- Report: "Packs should be versioned and selected against the engine (application) version."
- Context: the craft packs (`crates/craft/packs/*.toml`, `plumcraft_craft::manifest`). Just before this, the newly installed craft apps added command parameters (PhotoCraft 0.20.2, SoundCraft 0.17.1, VectorCraft), and the packs were edited to match on branch `craft-baselines`. A pack has no version of its own and doesn't say which application versions it fits, so one pack serves every installed version.
- Issue: #56
- Fixed: 2026-11-06, PR #58

### B16. Not everything tabular can be converted to a TABLE

- Reported: 2026-11-06
- Report: "anything tabular should be convertible to a TABLE (csv, grid objects, etc)"
- Context: Convert (`core.convert`, `docs/research/20-convert.md`) and the TABLE core type. Today the text engine converts Text ↔ Table and Array ↔ Table (6.3), and has CSV readers. Whether a CSV file value, a GridCraft workbook or range, or other tabular craft and pack types have a converter to TABLE is not checked yet.
- Cause: CSV files, CSV text, arrays and GridCraft workbooks (Export CSV → CSV to Table) already reached TABLE. JSON had no converter, so rows such as GridCraft's Read range went JSON → Text → Text to Table and were parsed as CSV. CSV readers knew only commas.
- Issue: #54
- Fixed: 2026-11-06, PR #55

### B15. Not every value can be turned into an array; a binary file can't be cut and reshaped

- Reported: 2026-11-06
- Report: "All values should be reducible to arrays, even if just a bytes array. value -> bytes array -> slice -> shaped array of dtype should be allow truncation of a binary file and casting it to a user determined shape."
- Context: core types and Convert (`core.convert`, `docs/research/20-convert.md`), and the array engine (`crates/array`). The chain asked for is any value → its bytes as a 1-D `uint8` ARRAY → a slice (to cut a binary file short) → a reinterpretation as an array of a chosen dtype and shape.
- Issue: #50
- Fixed: 2026-11-06, PR #53

### B14. A port's value isn't shown on hover unless a wire is connected; clicking a port doesn't watch it

- Reported: 2026-11-06
- Report: "ports should hover reveal the available value even if no wire is connected, click should watch the port value"
- Context: the node canvas (`crates/ui/src/canvas.rs`), seen while working in a private batch app's window on PlumCraft 0.4.0. Today a node's output is watched by clicking the node's title, which watches its default output (`run.watch {node, port?}`).
- Issue: #49
- Fixed: 2026-11-06, PR #52

### B2. No installer for Intel (x86_64) Macs

- Reported: 2026-11-05
- Report: "No installer for x86 arch on Mac"
- Context: releases carry only `PlumCraft-VERSION-macos-arm64.dmg`, as `packaging/release.sh` builds this machine's architecture. On 2026-11-05 `packaging/macos.sh` gained `CARGO_BUILD_TARGET=x86_64-apple-darwin` (uncommitted) and a test `PlumCraft-0.2.2-macos-x86_64.dmg` was built from the working tree, but it was never run on an Intel Mac and `release.sh` doesn't build or upload it.
- Fixed: 2026-11-05, PR #41. `release.sh` on a Mac builds both DMGs (`arm64` in `target/release`, `x86_64` with `CARGO_BUILD_TARGET`), checks both Rust targets are installed before building, and checksums and uploads both. `macos.sh` checks every binary's architecture with `lipo` and runs the Intel CLI when Rosetta 2 is installed. Both 0.2.3 DMGs built, signed and verified on the M4 Max. **Not yet run on an Intel Mac** (this Mac has no Rosetta 2).
- Issue: #23

### B12. No viewer for STL files

- Reported: 2026-11-05
- Report: "Add viewer for STL files"
- Context: the viewers (`crates/ui/src/viewers/`, registry in `mod.rs`) have nothing for 3D meshes, so an STL value gets the hex dump. The protocol already anticipates meshes: `Media::Mesh` (`crates/proto/src/catalogue.rs`), a `MeshLod` rendition (`crates/proto/src/display.rs:140`), and `model/stl` ↔ `stl` in `crates/proto/src/media.rs:32`. Not yet known: where the STL came from (a file, an engine such as SolidCraft's export) and whether it was ASCII or binary STL.
- Fixed: 2026-11-05, PR #40. A `mesh` viewer (`crates/ui/src/viewers/mesh.rs`) for values typed `model/stl`, binary or ASCII: flat-shaded on the CPU with a z-buffer, turned by a drag, zoomed by scrolling, fitted by a double-click or Fit. The bounded reader is `plumcraft_proto::mesh` and joins the hostile sweep. Both are adapted from SolidCraft (MIT, `THIRD-PARTY.md`) after research found nothing ready to use with egui 0.36 and wgpu. `MeshLod` renditions aren't used yet.
- Issue: #38

### B3. With automatic engines on, the Engine menu still offers every implementation

- Reported: 2026-11-05
- Report: "When \"preferences->engines->select engine automatically\" is enabled, the \"engines->select engine\" should just select the engine, not offer the list of implementations"
- Context: the setting is Settings › Engines › Pick Engines Automatically (`crates/ui/src/settings_view.rs:225`, `engine.auto`); the Engine menu's list of engines and their hostings comes from `engine_choices_menu` (`crates/ui/src/engine_view.rs:423`, called from `crates/ui/src/lib.rs:1509`). Expected: with the setting on, choosing an engine starts it the fastest way it can be hosted (in-process, then a process, then HTTP) without a submenu of implementations.
- Fixed: 2026-11-05, PR #39. With the setting on, `engine_choices_menu` shows each engine as one item running `engine.use` with no mode, which the session already resolves to the fastest hosting (`Engines::best`); with it off, the submenu stays. Tests: `crates/ui/tests/kittest_engine_menu.rs` (the automatic case fails without the fix).
- Issue: #24

### B11. `a_panicking_node_rebuilds_the_engine` raced the engine's rebuild on CI

- Reported: 2026-11-05
- Report: "Add as B11" (after the test failed the first CI run of both #30 and #31)
- Context: `crates/text/tests/conformance.rs`, `a_panicking_node_rebuilds_the_engine`: a node panics and the host rebuilds the engine. It failed the first runs of PR #30 and #31 (runs 42102535342, 42102561899) and both runs of #36 under cargo-nextest (42122511058, 42123346098), and passed on rerun and locally.
- First diagnosis, wrong: too short a 5 s wait.
- Cause: after the panicking run's reply, the test asserted `events.iter().any(EngineDown) || builds == 2`. The `EngineDown` half could never hold: the host pushes it as a session event (no run id), and `collect` polls the run's events only. The other half raced the host, which replies and then rebuilds (`crates/engine/src/host.rs`: `going_down`, `finish`, `rebuild`). Reproduced every time by making the rebuild take 300 ms before it counts; under load on the M4 Max it never failed in 40 runs.
- Fixed: 2026-11-05, branch `fix/33-panic-rebuild-race`. The test reads the session's events and asserts `EngineDown` comes before the run's reply, the order the host guarantees, then waits for the rebuild as before. The slow rebuild stays in the test, so the reply always comes first; 0 of 30 runs failed under load.
- Issue: #33

### B6. npexact has no contraction rule for numpy's runtime-dispatched SIMD loops

- Reported: 2026-11-05
- Report: "In PlumCraft, but remember as bug for future fix in npexact" (on fixing B5)
- Repo: npexact, in the Rust kernel library's repository; not PlumCraft's to fix
- Context: `npexact::random::Contraction::host_default()` (npexact 0.1.1, `src/random/mod.rs:223`) decides fused-or-plain by architecture at compile time: fused on aarch64, plain elsewhere. That fits numpy.random's C, which the wheels compile once, but not numpy's SIMD ufunc loops, which numpy picks at run time and which fuse on x86-64 with FMA3 and AVX2 (B5). PlumCraft borrowed the random rule for Mandelbrot's complex multiply and got x86-64 wrong. B5 is fixed in PlumCraft with its own `numpy_complex_fma()` (`crates/array/src/generate/noise.rs`); npexact should offer that rule (a runtime "numpy's SIMD contraction" beside `Contraction`, with fixtures from both hosts), and PlumCraft should then call it instead.
- Issue: the Rust kernel library's #10
- Fixed in npexact: 2026-11-05, the Rust kernel library's #11 (`5a51589`), with the recorder in the kernel library's #74. `npexact::complex::Mul` has `array_default()`, decided at run time (fused on arm64, and on x86-64 with FMA3 and AVX2), and `scalar_default()`, decided at build time: numpy scalars are plain on x86-64. npexact 0.1.2 published it (tag `npexact-v0.1.2`).
- Fixed in PlumCraft: 2026-11-05, branch `fix/b6-npexact-complex`. The Mandelbrot kernel calls `npexact::complex::Mul::array_default()` and `mul_c64` (npexact 0.1.2) in place of its own `numpy_complex_fma()`; the golden test passes on the M4 Max and on the Linux test host.

### B4. No built-in viewer for XML files

- Reported: 2026-11-05
- Report: "No build in viewer for XML files."
- Context: the built-in viewers are in `crates/ui/src/viewers/` (`basic`, `grid`, `pdf`, `plot`, `wrap`, `yaml`); there is a YAML one but nothing for XML. SVG, which is XML, is drawn as a picture (`crates/ui/src/media.rs`). Read here as "built-in"; no detail yet on where the XML came from (a file node, an engine's value) or what it showed instead.
- Issue: #26
- Fixed: 2026-11-05, branch `fix/26-xml-viewer`. A new `xml` viewer (`crates/ui/src/viewers/xml.rs`) takes XML text (a declaration, or an element after comments) and bytes typed `application/xml`, `text/xml` or `…+xml` (SVG keeps its own), ahead of Text. It shows an outline of elements, attributes, text and comments, with the source a checkbox away and a parse error's position. roxmltree parses it (already in the build through usvg), with DTDs refused, a node limit, and the outline bounded in depth, nodes and bytes; tests sweep truncations, flipped bytes and an entity bomb. Checked in the real editor with `claude-debug/xml_viewer_probe.py` (XML into the demo's `story` node, popped out with `viewer: "xml"`).

### B1. A popout covers the whole screen when the editor is full screen

- Reported: 2026-11-05
- Report: "popout when editor is full screen covers up whole screen"
- Context: the editor (`target/debug/plumcraft-editor`, working tree with the theme and Settings changes) on macOS in full screen; popouts are `crates/ui/src/popouts.rs`
- Issue: #22
- Cause: not a full-screen window of its own. With the editor full screen, macOS's automatic window tabbing made the new pop-out a tab of the editor's window, so it filled the screen and hid the editor. Reproduced in the real editor with `claude-debug/popout_fullscreen_probe.py` (full screen through the accessibility attribute, then `view.popout`): the tab bar showed "Demo — PlumCraft" and "shout.text — PlumCraft".
- Fixed: 2026-11-05, branch `fix/22-popout-fullscreen`. `no_window_tabbing()` (`crates/ui/src/lib.rs`, from `setup_context`) turns automatic tabbing off with objc2-app-kit's safe `NSWindow::setAllowsAutomaticWindowTabbing`; the probe then shows the pop-out as a 760×592 window of its own over the full-screen editor.

### B5. The Mandelbrot kernel doesn't fuse on x86-64, though numpy does

- Reported: 2026-11-05
- Report: "fix test data" (on the first Linux CI run failing `mandelbrot_matches_numba`)
- Context: CI run https://github.com/seantrue/plumcraft/actions/runs/42081191424 (PR #25, ubuntu-latest, x86_64): `crates/array/tests/generate.rs:85`, "mandelbrot 33×21: 9 of 693 pixels differ, max error 9.5e-1"; the 40×30 case matched. Cargo stopped at this test binary, so the rest of the Rust tests and the Python tests didn't run.
- First diagnosis, wrong: that the golden was arm64-only and x86-64 needed its own. On the maintainer's "Record mandelbrot test data on [the Linux test host]", the golden recorded on the Linux test host (x86-64, same numpy 2.5.3 and numba 0.68.0, identical numba kernel) matched the committed arm64 one in every pixel of all three cases (`claude-debug/mandelbrot_golden_host.py`).
- Cause: numpy's complex64 multiply fuses its multiply-adds on any CPU numpy dispatches its AVX2/FMA3 loop to, not just on arm64. `claude-debug/complex64_fma_probe.py` matched a fused model on 100% of 200,000 products on both machines and an unfused one on 71%. The kernel (`crates/array/src/generate/noise.rs:163`) picks fusion with `npexact::random::Contraction::host_default()`, a compile-time rule (fused on aarch64, plain elsewhere) that fits numpy.random's compiled C, not the runtime-dispatched SIMD loop. So on x86-64 it runs plain while numpy fuses. The test data is right; the kernel is wrong on x86-64.
- Fix: fuse when numpy's loop would: aarch64, or x86-64 with FMA3 and AVX2 at run time. The rule may belong in npexact, beside `Contraction`. The Linux test host and the CI runners both have FMA3 and AVX2.
- Issue: #27
- Fixed: 2026-11-05, PR #25 (`ba68b25`); #27 closed

### B7. The editor's audio tests time out on Linux CI

- Reported: 2026-11-05
- Report: found by the first Linux CI run with `--no-fail-fast` (run https://github.com/seantrue/plumcraft/actions/runs/42083352370); the maintainer asked to record and fix it ("Go for 2,3-5")
- Context: `apps/plumcraft-editor/src/lib.rs` `tests::audio_kittest`: `play_without_an_output_says_so_and_does_not_panic`, `a_1khz_tone_peaks_in_its_spectrogram_bin` and `the_audio_demo_runs_with_waveforms_and_an_audio_view` each panic at `lib.rs:2084` ("timed out waiting for the watched audio", "… the tone's spectrogram", "… the watched audio and its spectrogram") after 30 s. ALSA on the runner finds no sound card. The first test is meant to pass with no output device.
- Cause, found on the Linux test host: not the missing sound card (`ALSA_CARD=NoSuchCard` passes, as does 4 CPUs) but the runner's disk. commonkit's store declines to publish when the disk has under `MIN_FREE_GB` (8 GB) free, and after the workspace's build the runner has less, so the watched audio never arrives. With a fresh store root and `COMMONKIT_STORE_MIN_FREE_GB=100000`, the Linux test host times out the same three tests the same way. Same cause as B9.
- Fix: CI sets `COMMONKIT_STORE_MIN_FREE_GB=2` for the job. The store behaves as designed; that the editor waits rather than saying the value is unavailable may be worth a look.
- Issue: none
- Fixed: 2026-11-05, PR #25 (`ba68b25`), the CI floor

### B8. An SVG label in `sans-serif` draws nothing on Linux

- Reported: 2026-11-05
- Report: as B7 ("Go for 2,3-5")
- Context: `crates/svg/src/lib.rs:169` `text_is_drawn_with_the_systems_fonts`: "0 inked pixels" on ubuntu-latest, although `fonts()` found fonts (the test returns early when there are none). The label is `<text font-family="sans-serif">Plum</text>`; on the Mac it draws.
- Cause, likely: fontdb takes the generic families from fontconfig on Linux (on the Linux test host `sans-serif` is FreeSans, installed, so it passes there), and nothing checks that the family it names is installed. Where it isn't, the label matches no face and isn't drawn. Not reproduced off the runner; the next CI run's failure message names the family if this is wrong.
- Fix: `installed_generics` in `crates/svg/src/lib.rs` points each generic family that names a missing family at an installed one, with a test that sets them all to a missing family.
- Issue: none
- Fixed: 2026-11-05, PR #25 (`ba68b25`); the next Linux CI run passed it

### B9. Large JSON crosses the Python binding as `opaque`, not `blob`, on Linux

- Reported: 2026-11-05
- Report: as B7 ("Go for 2,3-5")
- Context: `python/tests/test_audio.py:69` `test_large_json_crosses_as_a_blob`: `assert ('opaque' == 'blob')` at line 77 on ubuntu-latest; 217 of 218 Python tests pass there, and all pass on the Mac.
- Cause: the same as B7. The store declines the large JSON because the runner's disk is under commonkit's 8 GB floor, so `Plane` reports it `opaque` (`python/src/plumcraft_engine/data.py:239`). Reproduced on the Linux test host with a fresh store root and `COMMONKIT_STORE_MIN_FREE_GB=100000`; without the raised floor it passes.
- Fix: as B7.
- Issue: none
- Fixed: 2026-11-05, PR #25 (`ba68b25`), the CI floor
