# N1 slice 4: the core asks, the shell answers

Branch `native/n1-asks`, working tree only: nothing committed, staged or stashed. 2026-11-19.

## Summary

- **New in the core: `plumcraft_editor_core::asks`.** A per-document queue of asks `{ask, kind, modal, by, params}`. Each ask is raised with a continuation and answered once, or cancelled, withdrawn or timed out. Every answer is checked as hostile input.
- **New in the core: `plumcraft_editor_core::invoke`.** The logic of `EditorApp::invoke`, `answer_prompt`, `go_on`, `ask_where_to_save`, `browse_for_path`, Choose an App…, theme import/export, forms and paste, as a continuation state machine over a `Host` trait.
- **The egui shell answers asks with what it had** (`crates/ui/src/asks_view.rs`):
  - a person's file asks go through the unchanged `FileDialogs` trait, at once (rfd or a test's fakes);
  - prompts and forms go to their existing windows;
  - the clipboard is egui's;
  - `open.external` is spawned as the session spawned it before.
- **Control channel (decision 1).**
  - `ui.invoke` of a command that needs a file replies at once, with the ask in `ui.inspect`'s new `asks` list.
  - The new `ask.answer {ask, answer}` (or `{ask, cancel: true}`) answers it.
  - `ui.prompt` is unchanged.
  - `docs/control-protocol.md` is updated.
- **Item 5 is done.**
  - Copy goes through a `clipboard.set` ask, and Edit › Paste through a `clipboard.get` ask; ⌘V stays shell-initiated.
  - The session's app starts (`/usr/bin/open`, `xdg-open`, a craft's app) go through `open.external` asks, by a session hook.
- **FFI.** Asks appear in `plum_poll`'s output (`asks`, `asksEnded`), and `ui.invoke` and `ask.answer` work as requests. Both are tested through the C ABI, and the C function list is unchanged.
- **Tests:**
  - 1,320 ran before and 1,336 after, with the same single expected failure in two b3 runs and one md5 run: 16 new tests, none gone.
  - **No existing test file was edited.**
  - Python: 224 passed under md5 and b3; ruff is clean.
  - Gates: fmt, clippy, the hash gates and the toolkit-free gate are all clean.
- **PlumImpress** (scratch copy) checks with no source change, and its `open_recipe` test, which installs fake `FileDialogs`, passes.
- **One mishap to fix by hand: my first probe run wrote to the maintainer's real editor preferences.** See "Driving the real editor".

## The ask model as built

| Aspect | As built |
|---|---|
| Shape | `{ask: u64, kind, modal, by: "person" \| "script", params}` (`Ask::to_json`) |
| Kinds | `file.open`, `file.save`, `folder.pick`, `prompt`, `form`, `text`, `clipboard.set`, `clipboard.get`, `share`, `open.external` (the `Kind` enum; `text` and `share` exist but nothing raises them yet) |
| Modal | `file.*`, `folder.pick`, `prompt`, `form`, `text` |
| Answers | `file.open` → `{paths: [..], grants?}` (one path unless `multiple`); `file.save`, `folder.pick` → `{path, grant?}`; `prompt` → `{answer}`, one of `params.answers`; `form` → `{values: {key: text}}` (keys must be the form's); `text`, `clipboard.get` → `{text}`; `clipboard.set`, `share`, `open.external` → `{}`, `{notice}` or `{error}` |
| Cancel | `Asks::cancel(id)`; over the channel `ask.answer {ask, cancel: true}` |
| Lifetime | Raised with a continuation (`invoke::Cont {then, remember}`); the first answer or cancel ends it. Ended ids are remembered (the last 256), so a late answer is told "ask 3 was already answered/cancelled/withdrawn/timedOut" and not just "unknown" |
| Withdrawal | A newer ask of the same kind withdraws an open one, for the modal kinds and `clipboard.get`. This is how today's single `prompt` field was overwritten. A `file.*` ask for a path input is withdrawn when its node is removed (`invoke::tidy`, each frame and after each FFI request). Past 64 open asks the oldest is withdrawn. `withdraw_all` is there for a document closing; today one editor is one document and dropping it drops its queue |
| Timeouts | Only `clipboard.get`, after 5 s: an empty clipboard sends no Paste event. Nothing a person answers times out |
| Who presents | `by: person` (a menu, shortcut or button started the chain) or `script` (`ui.invoke` or `ask.answer` started it). A chain keeps its starter's `by` all the way: the save dialog after a prompt's Save belongs to whoever started the New |
| Feed | `Asks::take_feed()` returns what was raised and what ended since the last call (the FFI's `asks` and `asksEnded`); a `generation` counter bumps on every change |
| Hostile input | A wrong shape, unknown fields, non-strings, empty or too-long paths (over 4,096 bytes), NUL, several paths for a single ask, an answer not in `answers`, form keys not in the form, text over 64 MiB, a non-numeric id, `answer` together with `cancel`, and `cancel` not `true` are all refused with a reason. A wrong shape leaves the ask open. Tests: `asks::tests::hostile_answers_are_refused_and_leave_the_ask_open`, `invoke::tests::hostile_answers_run_nothing`, and the hostile loops in `kittest_asks` and `ffi::asks` |

### Differences from the draft contract (`docs/shell-interface.md` section 11)

| Draft | Built | Why |
|---|---|---|
| `{ask, doc, kind, modal, params}` | No `doc`; `by` added | One queue per document, and documents have no ids yet. `by` is what lets the egui shell know which asks it presents itself (decision 1) |
| `file.save` answer `commit: true` streams a scratch file | Refused: "commit … isn't supported yet" | No shell needs it yet; it comes with Android |
| `startIn`: the purpose's last folder, else the document's folder, else `dirs.documents` | The purpose's last folder (`prefs.dirs`) or null; `current` (the workflow's file) and `suggested` (Save Result's folder and name) carry the rest, as the rfd dialogs used them | Kept exactly what each dialog started in before. `dirs.documents` doesn't exist until the core's config does |
| Params as listed | Extra keys a desktop shell uses: `current`, `hint` (the `PathHint`), `formats` (the `SaveFormat`s), `suggested`, `title`. A prompt gains `title` and `then` | `FileDialogs`' methods take these. The extra keys are harmless to other shells |
| `form`: on an error the core "raises the form again with `error`" | The same ask is reopened with its id and `params.error` | The window keeps its identity, and a script keeps the id it holds |
| While a modal ask is open, shortcuts wait | Shortcuts wait only while a **prompt** is open, as before | Otherwise a script's open file ask would freeze a person's keyboard |
| Attach… and Install Engine… become `CommandForm`s | They stay egui windows (`Host::open_window`) | A UI change, outside this slice. The FFI, with no window, runs the command as it is |
| Rename… and the other naming windows become `text` asks (question 4) | Not done: still egui windows. The `Text` kind and its answer parsing exist | `ui.inspect`'s `rename` and the rename kittests (which edit `rename.text` in place) belong with the `ui.*` slice |
| `share` from Send To | The kind exists; the egui shell answers `{error: "share isn't available here"}`; nothing raises it | As the brief allowed |
| `screenshot` | Not an ask yet | It belongs with the `ui.*` slice (`ui.screenshot`) |
| Grants stored in recent files | Accepted in answers and ignored | Recent files don't exist yet |

## The invoke state machine

`invoke::invoke(h, id, by)` fills in what the UI knows and runs the command through `Host::run`, which is the egui shell's `try_run` (engine switches included). Where it needs the platform, it raises an ask and returns. `invoke::answer(h, id, value)` checks the answer and remembers the folder (`Cont::remember` → `prefs.remember`), then carries the chain on (`go`). `invoke::cancel` ends a chain (a prompt's cancel is its Cancel). The `Host` trait has five required methods (`session`, `session_mut`, `asks`, `run`, `say`); the canvas ones (`selection`, `group_to_ungroup`, `canvas_center`, `measured`, `select_result`, `fit`, `frame_selection`, `paste_pos`) and the window ones (`close`, `closing`, `open_window`, `allow_uploads`, `take_held_run`, `theme_name`, `prefs`, `take_launches`) have defaults, so the FFI's core implements only the five.

| Trigger | Asks raised, in order | Final command |
|---|---|---|
| New (`graph.new`), clean | none | `graph.new {discard: false}` |
| New over unsaved changes | `prompt unsavedChanges (then: new)` → discard | `graph.new {discard: true}` |
| New over unsaved changes, Save, graph has a file | `prompt` → save | `graph.save`, then `graph.new {discard: false}` once not dirty |
| New over unsaved changes, Save, no file | `prompt` → save → `file.save {purpose: workflow}` → path | `graph.saveAs {path}`, then `graph.new` once not dirty |
| Open (`graph.open`), clean | `file.open {purpose: workflow}` | `graph.open {path, discard: false}` |
| Open over unsaved changes | `prompt (then: open)` → discard → `file.open` | `graph.open {path, discard: true}` (or via Save as for New) |
| Quit (`app.quit`, Cmd+Q, the close button) over unsaved changes | `prompt (then: quit)` → discard, or save (→ `file.save` without a file) | the window closes (`Host::close`), after `graph.saveAs`/`graph.save` for Save |
| Save (`graph.save`), graph has a file | none | `graph.save` |
| Save of an unsaved graph, Save As | `file.save {purpose: workflow, name, current}` | `graph.saveAs {path}` |
| Export Bundle (`graph.bundle`) | `file.save {purpose: bundle, name: <stem>.zip}` | `graph.bundle {path}` (its "bundled, but…" note as before) |
| Insert, Insert as Group | `file.open {purpose: workflow}` | `graph.insert`/`graph.insertGroup {path, pos: canvas centre}`, the result selected, "inserted; n wire(s)…" |
| Theme Import…, Export… (Settings) | `file.open`/`file.save {purpose: theme, name: <theme>.plumtheme.json}` | `theme.import`/`theme.export {path}` |
| Open With › Choose an App… | `file.open {purpose: app, startIn: /Applications}` | `value.open {…, app}` |
| Save Result…, Contact Sheet… (`run.save {ask: true}`) | `file.save {purpose: result, kinds: the engine's save formats, name: save_cmds::suggested_name, suggested}`, or "can't save: why" | `run.save {…, path}` (relative to the workflow's folder where inside it) |
| A path input's "…" | `file.open`, `file.save` or `folder.pick {purpose: path}` by the hint's mode | `node.set {value: paths::chosen_path}` |
| A `files` input's Select Files | `file.open {purpose: files, multiple: true}` | `node.set {value: one path per line}` |
| Review before running (`run.awaitingReview`) | `prompt reviewWorkflow (detail: the Review)` | trust → `graph.trust`; cancel → `run.decline_review()` |
| A run or save that would upload files | `prompt allowUploads (detail: [{to, file}])` | trust → `allow_uploads` and the held run resent; cancel → the held run cancelled |
| A command with a `CommandForm` | `form {command, fields, values (prefilled), submit}`; refused → the same ask reopened with `error` | the command with the fields' params |
| Copy, Cut, `viewer.recipe {copy: true}` | `clipboard.set {text, kind: workflow \| text}` | none |
| Edit › Paste | `clipboard.get {kinds}` (5 s) | `edit.paste {text, pos}`, the result selected |
| `doc.open`, `value.open` (Open in …, Open With) | `open.external {path, app?, program, args}` | none: the shell starts it |
| `group.create`, `node.expand`, `group.ungroup`, `frame.add`, `graph.tidy`, `node.remove`, `node.duplicate`, an application's command | none: the selection, measured sizes or `group_to_ungroup` are filled in, as before | the command; "select … first" and an application's `notice` go to the status bar |
| Engine › Attach…, Install Engine… | none: the shell's window (`Host::open_window`) | as before |

**Composing with slice 3's engine switches.** An answer runs its command through `try_run`, so a switch it asks for (an auto-picked engine after `graph.open`, or a form for `engine.switch`) sets `engine_asked`. `ask.answer` is in `handle_control`'s frame-mode list beside `command.run`, `ui.invoke` and `ui.prompt`, so its reply is parked until the switch is done. `kittest_asks::an_answer_that_switches_engines_replies_once_switched_and_no_frame_waits` tests this:

- a form for `engine.switch` is answered with `process` against a fake that takes 500 ms to start;
- the reply comes once the switch is done, with `engine.pending: false` and a process label;
- the longest frame stayed under 250 ms.

In the FFI, `ui.invoke` and `ask.answer` park on `SwitchWait` (with a new `asks` flag) and reply with the asks once the switch is done.

## How the egui shell answers each kind

| Kind | Person's | Script's |
|---|---|---|
| `file.open`, `file.save`, `folder.pick` | At once, through `FileDialogs`. `purpose` picks the method: `open_workflow`, `save_workflow(current)`, `save_bundle(current)`, `pick_app`, `save_output(formats, suggested)`, `open_theme`, `save_theme(name)`, `pick_files(hint, current)`, `pick_path(hint, current)`. `start_in(startIn)` is called first where the ask has `startIn` (not for `app`), exactly where `ask_dialog` called it. A dismissed dialog cancels the ask | Not shown. A small "Waiting for a script" window says `A script is answering "Save Workflow" (ask 1).`, with **Choose…** (the native dialog, for a person to take it over) and **Cancel** |
| `prompt` | The existing prompt modals. `EditorApp::prompt` is now a mirror of the open prompt ask, set by `sync_asks` | The same: the modal shows, and a script answers with `ask.answer` or `ui.prompt` |
| `form` | The existing form window (`FormDialog`, with a new `ask` field). Submit answers `{values}`; a refusal keeps the window open with the reopened ask's `error`; Cancel or Esc cancels the ask; the window closes when its ask ends | The same window. It closes when the script answers |
| `clipboard.set` | `ctx.copy_text`, and `copied` is set; answered `{}` | The same |
| `clipboard.get` | `ViewportCommand::RequestPaste`; the Paste event answers `{text}` | The same |
| `open.external` | `doc_cmds::spawn_app(program, args, path)`; a failure is answered `{error}`, which goes to the status bar | The same |
| `share` | `{error: "share isn't available here"}` | The same |
| `text` | Not raised | |

`FileDialogs` is unchanged (its docs now say it is how this shell answers file asks), so the five test files and PlumImpress test that install fakes compile and pass as they are. A shell with deferred pickers needs nothing like the spike's `waiting()`, `take_answer()` or `SendTo`: it leaves the ask open and calls `ask.answer` (Rust: `EditorApp::answer_ask`; C: the `ask.answer` request) when its picker returns.

## `ui_asks`, the clipboard and other apps

- **`Views::ui_asks`: bridged in reasoning, not absorbed.**
  - It is not a platform ask. The appearance commands (`view.textSize`, `theme.*`, `app.settings`) leave a request that `EditorApp::window_command` carries out inside the same `try_run`. The command's reply is built from the result (`app.settings` returns the settings; `theme.import` its theme).
  - Routing these through the asynchronous queue would make `command.run view.theme` reply before the theme is applied. That changes control replies, which the gate forbids.
  - Its right end is to move the appearance work into the core: themes and appearance are already core data (`editor-core/theme.rs`, `appearance.rs`). The window would then only restyle, and `ui_asks` would disappear. That belongs in the `ui.*` / menu-model slice.
  - Nothing else uses it, so there is still only one queue of things the core asks the platform for.
- **Clipboard: done.**
  - Copy and Cut (`edit_view::copy_selection`) and `viewer.recipe {copy: true}` raise `clipboard.set`.
  - Edit › Paste raises `clipboard.get`, and the next Paste event answers it.
  - **⌘V stays shell-initiated:** egui reads the pasteboard and sends `Event::Paste` on its own. Then the shell offers the text to the core (`invoke::paste`, `edit.paste` at the pointer), as the draft allows ("a shell that reads the pasteboard itself sends `edit.paste` directly").
  - Copy-to-clipboard inside viewers (`viewers/basic.rs`: a text viewer's Copy) stays a direct egui call, being viewer-local.
  - No behaviour change: `copy_and_paste_through_the_clipboard_events` and the moves test pass unedited.
- **Opening other apps: done, through a session hook.**
  - `DocApps` gains `set_launcher` and `launches_by_default`.
  - The core's `invoke::route_launches(&mut Session)` installs a launcher that records `(program, args, file)` and returns `Ok`. It does so only when the session still spawns by default, so a test's or an application's launcher is kept.
  - `raise_launches` turns the records into `open.external` asks.
  - `EditorApp::new` and the FFI's `Core::open` route their sessions. The session does not depend on the core.
  - **Behaviour change:** a spawn that fails (say `/usr/bin/open` missing) now shows in the status bar a moment later, instead of being `doc.open`'s or `value.open`'s error. `docs.last` and `recent_apps` record the open before the start. On success, nothing differs.
  - A downstream application that replaces `session.docs` wholesale after `EditorApp::new` goes back to spawning directly, which is today's behaviour.

## Control channel

| Change | Detail |
|---|---|
| `ui.invoke` | Runs `invoke_by(id, By::Script)`. What it needs is asked and the reply comes at once, as `ui.inspect` plus `invoked`. No native dialog is shown for a script's ask |
| `ask.answer` (new) | `{ask, answer}` or `{ask, cancel: true}`; it replies with the UI state. Unknown fields, a bad id, both or neither of `answer` and `cancel`, and every refusal from `Asks` are errors. It is parked until an engine switch it caused is done |
| `ui.inspect` | New `asks`: `[{ask, kind, modal, by, params}]`. `prompt` is kept, byte for byte, as the mirror of the open prompt ask |
| `ui.prompt` | Unchanged: `invoke::answer_prompt` answers the open prompt ask. A chain a person started (Cmd+O, then `ui.prompt discard`) still shows the native dialog, as its button would; that is what `kittest_files` checks |
| Docs | `docs/control-protocol.md`: the `ui.invoke` row (the timing change and the "Waiting for a script" note), the `ui.prompt` row (a convenience over `ask.answer`, and who owns the follow-on ask), a new `ask.answer` row, `asks` in the example and in the field list with every kind's params and answers |

**Probes and tests that used the old `ui.invoke` behaviour.** I searched `claude-debug/` (`*.py`, `*.rs`, `*.sh`, `*.md`), `python/`, `crates/`, `apps/`, `scripts/` and `docs/`:

- **None uses `ui.invoke` with `graph.open`, `graph.save`, `graph.saveAs`, `graph.bundle`, `graph.insert*`, theme or `run.save`.** The blocking path had no script user, as decision 1 expected.
- Uses that remain, all unaffected:
  - `ui.invoke graph.tidy`: 23 calls across the `claude-debug` probes and `docs`;
  - `ui.invoke app.quit`: `claude-debug/vary/p08_followup.py` and `kittest_files` (prompt or close, unchanged);
  - `ui.invoke graph.new`: `kittest_files`, the prompt, unchanged;
  - `ui.invoke engine.reconnect`: `engine_thread.rs`, the default arm, unchanged;
  - `ui.invoke` of a command that doesn't exist: an error, unchanged.

## FFI evidence

`crates/ffi/tests/asks.rs`, through the C ABI (`plum_open`, `plum_request`, `plum_poll`, `plum_close`, with a poll loop of its own that keeps the feed's `asks` and `asksEnded`):

- **`save_as_raises_a_file_ask_that_a_shell_answers_later`:**
  - `ui.invoke graph.saveAs` replies with a `file.save` ask (`by: script`, `purpose: workflow`), which also arrives in the poll's `asks`.
  - The core answers `ping` meanwhile.
  - Five hostile `ask.answer`s are refused.
  - The path answer saves the file, and `asksEnded` carries `{ask, ended: "answered"}`.
  - A second answer gets "already answered".
- **`new_over_unsaved_changes_goes_through_the_prompt_a_save_and_a_path`:**
  - `ui.invoke graph.new` raises `prompt unsavedChanges (then: new)`.
  - Answering `save` raises `file.save (name: untitled.json)`, with the node still there.
  - The path saves the file (it contains `text.literal`), then the graph is new (`graph.get` shows no nodes), with two `answered` ends.
  - Then New is cancelled with `cancel: true` and keeps the node.
  - An unknown command and an unknown field are errors.

`plum_poll`'s JSON gains `"asks"` and `"asksEnded"` only when there are some, so every existing poll consumer sees exactly what it saw before. The C function list and `PLUM_ABI_VERSION` are unchanged (`header.rs` passes).

## Tests

| Run | Ran | Passed | Failed | Skipped | Failing |
|---|---|---|---|---|---|
| Baseline b3 (`s4-base-b3`) | 1,320 | 1,319 | 1 | 10 | `plumcraft-craft::params_coverage hidden_params_match_the_baseline` |
| Final b3, run 1 (`s4-final1-b3`) | 1,336 | 1,335 | 1 | 10 | the same |
| Final b3, run 2 (`s4-final2-b3`) | 1,336 | 1,335 | 1 | 10 | the same |
| Final md5 (`s4-final-md5`) | 1,336 | 1,335 | 1 | 10 | the same |

- **Test names** (`s4-base-names.txt` against `s4-final1-names.txt`): the final runs add exactly these 16 tests, and none is gone.
  - `asks::tests` ×5.
  - `invoke::tests` ×5, including `new_over_unsaved_changes_prompts_then_saves_through_a_file_ask_then_starts_over` and `starting_an_app_is_an_open_external_ask_unless_a_launcher_was_given`.
  - `plumcraft-ffi::asks` ×2.
  - `plumcraft-ui::kittest_asks` ×4:
    - `new_over_unsaved_changes_saves_through_the_dialog_as_a_person_would`: File › New Graph clicked, the prompt, Save clicked, the fake save dialog, saved, then 0 nodes;
    - `a_script_answers_the_file_ask_that_ui_invoke_raises`;
    - `an_answer_that_switches_engines_replies_once_switched_and_no_frame_waits`;
    - `edit_paste_asks_for_the_clipboard_and_the_paste_event_answers`.
- **No `LEAK`, and no `shm_foreign_digest` or `shm_bad_key` marks.**
- **`cargo test` (threads, not nextest)** of the new FFI and kittest binaries and the core's lib passes.
- **Python, as CI runs it:** 224 passed under md5 and under b3, no bad marks; `ruff check`: clean.
- **Gates:**
  - `cargo fmt --all --check` and `cargo clippy --all-targets --all-features -- -D warnings`: clean.
  - `scripts/check-hash-gates.sh`: ok.
  - `scripts/check-core-toolkit-free.sh`: ok.

## Existing test files touched

None. Every existing file under `crates/*/tests`, `apps/*/tests` and every `#[cfg(test)]` module is as it was. The decided `ui.invoke` change forced no edit: no test invoked a dialog command through the control channel.

## Driving the real editor

`claude-debug/native-n1/s4_probe.py`; output in `s4-probe.txt` and pictures in `s4-probe/`. It runs `target/debug/plumcraft-editor --control 0` with the text engine in-process and a scratch preferences file, waits on the log and token file with hard limits, and the editor quits through `ui.invoke app.quit` (its PID is captured). What I saw:

1. **`ui.invoke graph.saveAs` replied in 6 ms** with `asks: [{ask: 1, by: "script", kind: "file.save", modal: true, params: {purpose: "workflow", title: "Save Workflow", name: "untitled.json", kinds: [{label: "Workflow", extensions: ["json"]}], current: null, startIn: null}}]`.
   - `ui.inspect` showed the same ask.
   - The window screenshot (`s4-probe/s4-waiting.png`) shows the graph and, at the bottom right, "Waiting for a script: A script is answering "Save Workflow" (ask 1)." with Choose… and Cancel.
   - No native dialog appeared.
2. **`ask.answer {ask: 1, answer: {path: …/s4-saved.json}}`** replied in 17 ms. The file exists, the title changed from "Untitled • — PlumCraft (…)" to "s4-saved.json — PlumCraft (Text · Rust · in-process (mode 1))", and `asks: []`.
3. **Hostile answers:**
   - the same ask again: "ask 1 was already answered";
   - ask 99999: "no ask 99999 is open";
   - `{ask: "x"}`: "ask must be an ask's id (ui.inspect's asks)".
4. **`ui.invoke graph.new` over an unsaved edit, with the graph saved to a file:**
   - it replied with the `prompt` ask (`then: "new"`), and `ui.inspect`'s `prompt` was as before;
   - `ask.answer {answer: "save"}` saved to the file (it now holds `text.upper`) and made the new graph (0 nodes), in 26 ms.
5. **The same over a graph with no file:**
   - Save raised `file.save` with `startIn` set to the probe folder (remembered from step 2);
   - answering a path saved `s4-second.json` and made the new graph;
   - `prefs.dirs` held only `workflow.write`, in the scratch preferences.
6. **`ui.invoke app.quit` over changes, then `ui.prompt {answer: "cancel"}`:** `closing: false`. After `graph.new {discard: true}`, `ui.invoke app.quit` closed the editor (exit 0).

**My mistake on the first run, which the maintainer should fix by hand.**

- **What went wrong:** the first run set `PLUMCRAFT_PREFS` instead of the editor's own variable, `PLUMCRAFT_EDITOR_PREFS`. So the editor used the real `~/Library/Application Support/plumcraft-editor/prefs.json`.
- **What changed there:**
  - `dirs.workflow.write` became `/Users/seant/workspace/plumcraft/claude-debug/native-n1/s4-probe`. I don't know what it was before, so I left it; the maintainer may want to reset or remove that key.
  - `trusted.json` beside it gained entries for the two workflows the probe saved.
- **The fix:**
  - The probe now sets `PLUMCRAFT_EDITOR_PREFS`.
  - I ran it again with checksums of both files taken before and after. They were unchanged (`s4-userprefs-before.txt`), and the scratch folder got its own `s4-prefs.json` and `trusted.json`.
  - The output above is from that second run.

## Downstream effects

**PlumImpress.** Copied without `target`, `.git`, `dist` or `claude-debug` to `claude-debug/native-n1/s4-plumimpress-check/`, with slice 3's `.cargo/config.toml` patching every `plumcraft-*` crate to this tree.

- `cargo check --tests` exits 0 with no source change.
- `cargo test -p plumimpress --test open_recipe`, which installs fake `FileDialogs` and opens through File › Open and Save Result, passes 2 of 2.
- The repository was not touched.

**API changes a downstream application would see:**

- **Moved:** `Prompt`, `AfterDiscard` and `PromptAnswer` moved to `plumcraft_editor_core::invoke` and are re-exported from `plumcraft_ui` under the same names. `Prompt` gains `title()`, `answers()` and `params()`.
- **New on `EditorApp`:**
  - pub field `asks`;
  - `invoke_by(id, By)`, `answer_ask(id, &Value)`, `cancel_ask(id)`, `present_asks()`;
  - `impl plumcraft_editor_core::invoke::Host for EditorApp`.
- **Re-exported from `plumcraft_ui`:** `AskId` and `By`.
- **Changed:**
  - `EditorApp::prompt` is still a pub field, but it is now derived from the asks each time they change. Writing it directly no longer opens a prompt; use `invoke::raise_prompt`. No code in either repository writes it.
  - `EditorApp::new` now routes the session's app starts (above).
- **Gone (private or `pub(crate)` only):** `copy_out`, `ask_dialog`, `go_on`, `ask_where_to_save` and `open_form`.
- **Forms and edits:**
  - `form_view::FormDialog` gains pub `ask: Option<AskId>`. Its fields were already partly private, so no downstream struct literal breaks.
  - `edit_view::EditorApp::edit_op`'s `ctx` parameter is now unused (`_ctx`); its signature is unchanged.
- **New in `plumcraft_session::doc_cmds::DocApps`:** `set_launcher` and `launches_by_default`.

## What later slices need to know

- **The `ui.*` methods into the core:**
  - `ui.prompt` and `ask.answer` are now thin over `invoke::answer_prompt` and `invoke::answer`. `ui.invoke` is `invoke::invoke(…, By::Script)`.
  - What still needs the egui `EditorApp` for `ui.inspect` is the canvas and windows: selection, `rename`, popups, parameters and the like. `asks` and `prompt` come from the core already.
  - The FFI has `ui.invoke` and `ask.answer` but no `ui.inspect`. Its replies carry `asks` and `lastError`.
  - When `ui.inspect` moves, rename becomes a `text` ask (the kind and its parsing exist; `Then` needs a `Rename` arm).
  - `Views::ui_asks` should be retired by moving appearance into the core, not by routing it through `Asks`.
  - `in_frame` and `engine_asked` remain the egui shell's way of saying "may not wait"; `ask.answer` joined the list that parks.
- **The menu model:**
  - `invoke` is per id and data-free except for `Host`. A menu model can mark which ids raise asks.
  - `Host::open_window` (Attach…, Install Engine…) is where those windows would become `CommandForm`s.
  - The "Waiting for a script" note is the only new piece of UI.
- **The media crate:** nothing here touches `resolve`. `open.external` copies are still made on the UI thread by the session (`copy_value`), as before.
- **Startup without waiting for the engine:** a `file.open` answered before the first catalogue arrives runs `graph.open` against whatever catalogue is there. The `Startup` step from slice 3 should hold person asks, or `graph.open`, until the catalogue is in.
- **Several documents:** `Asks` is per `EditorApp` and per FFI core. A `Document` in the core should own one, and closing a document should call `withdraw_all`.
- **Scope left out:**
  - `commit: true` streaming saves;
  - grants in recent files;
  - `startIn`'s fallbacks to the document's folder and `dirs.documents`;
  - the `screenshot` and `share` asks;
  - shortcuts waiting on every modal ask (deliberately not done: see the differences table).

## Lines of code

From `git diff --numstat`, plus the new files. `docs/BUGS.md` also shows as modified, but not by me: entries B27 and B28 appeared during this session from elsewhere, and I left them alone.

| Area | Added | Removed | Notes |
|---|---|---|---|
| `crates/editor-core/src/asks.rs` | 573 | 0 | New. 109 lines of tests |
| `crates/editor-core/src/invoke.rs` | 954 | 0 | New (after rustfmt). 159 lines of tests |
| `crates/editor-core/src/lib.rs` | 2 | 0 | |
| `crates/ui/src/asks_view.rs` | 323 | 0 | New: the `Host` impl, presentation, the `FileDialogs` adapter, the script note |
| `crates/ui/src/lib.rs`, `control.rs`, `edit_view.rs`, `form_view.rs`, `settings_view.rs` | 140 | 368 | `invoke`, `answer_prompt`, `go_on`, `ask_where_to_save`, `browse_for_path`, the prompt types and `open_form` left for the core |
| `crates/session/src/doc_cmds.rs` | 16 | 1 | The launcher hook |
| `crates/ffi/src/core.rs` | 167 | 6 | Asks, `ui.invoke`, `ask.answer`, the feed, `SwitchWait.asks` |
| `crates/ui/tests/kittest_asks.rs` | 249 | 0 | New |
| `crates/ffi/tests/asks.rs` | 125 | 0 | New |
| `docs/control-protocol.md` | 13 | 3 | |
| **Total** | **2,562** | **378** | Net +2,184, of which about 640 are tests |

## What the docs should say

- **`docs/STATUS.md`:** N1 slice 4 is done.
  - The core asks and the shells answer. `asks` and `invoke` are in `plumcraft-editor-core`, the egui shell answers with its dialogs and windows, and the FFI carries asks in `plum_poll` with `ui.invoke` and `ask.answer` as requests.
  - Decision 1 is in: `ui.invoke` replies at once, and scripts answer with `ask.answer`.
  - The clipboard and `open.external` go through asks.
  - Test counts 1,320 → 1,336, with the same single local failure.
- **`docs/research/24-native-shells.md`:**
  - **Section 16:** a slice 4 bullet with the above:
    - the `by` field and the rule that a chain belongs to whoever started it;
    - `ui_asks` left as it is, for the reason given here;
    - the open.external hook and its error-timing change;
    - rename, `screenshot`, `share`, `commit` and grants still to do.
  - **Section 6:** the table's `file.save` "`file.commit` when done" is not built.
  - **Section 4.1's row for `invoke`/`try_run`:** done, as `invoke::Host`.
- **`docs/shell-interface.md`:**
  - **Section 11.1:**
    - drop `doc` until documents have ids, and add `by`;
    - shortcuts wait only on a `prompt` (or decide otherwise);
    - add timeouts (`clipboard.get` after 5 s), the bound of 64 open asks, and the 256 ended asks that are remembered;
    - a refused form keeps its ask id.
  - **Section 11.2:** the extra params (`current`, `hint`, `formats`, `suggested`, a prompt's `title` and `then`); `open.external`'s `program` and `args`; `commit: true` refused for now.
  - **Section 11.3:** `startIn` is the remembered folder or null for now.
  - **Section 11.4:** Attach and Install stay windows for now; rename is not yet a `text` ask; ⌘V is the shell offering `edit.paste`.
  - **Section 5.2 or 7.1:** `plum_poll`'s optional `asks` and `asksEnded`.
  - **Section 17:** questions 1 and 19 are built.
- **`docs/control-protocol.md`:** done in this change.
