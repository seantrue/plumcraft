# Save Result, second part: contact sheets, no recipes, the name and graph hash

*2026-11-22, branch `feat/sdk-save-2` from `origin/feat/native-n0` (75707b4, after PR #165), uncommitted.*

The maintainer's decisions (note 24, three tables): the twelve cards ("1A; 2A; 3C \"No embedding of recipes. It was ill conceived.\"; 4A; 5A; 6A \"Also question saving non image shapes (mindim < 64, channels > 4, extreme aspect ratios)\"; 7A; 8A; 9A; 10A; 11A; 12B"); how far 3C reaches ("everywhere. Let's store a name and a hash of graph instead. Purely informational."); and the floor conflict that raised ("go with 2 less risk.": recipes stay with engines that track lineage, nothing else carries one).

## What was done

- **Decision logs.** Note 24 has all three exchanges, verbatim. In `docs/BUGS.md`: B118 is closed as not the app's (11A); B60 now records its condition (10A: close once a device run with the window set passes); B97 is approved (7A, needs the Windows test VM).
- **2A, contact sheets.** Both SDKs' `png` and `jpeg` formats make contact sheets (`Format::sheets`). A list saved with `sheet` becomes one picture: the layout is in `docs/interface.md` section 15, scaling is an integer box average, alpha is composited over white, labels go in the reply rather than on the picture, and the limit is 256 items. Rust and Python make the same pixels, checked against a shared fixture (`crates/engine/tests/fixtures/sheet.json`, the sheet's shape and SHA-256). The bytes are not identical: each SDK's PNG encoder compresses differently. Any other format refuses `sheet` on a list with `config`.
- **3C and option 2.**
  - The SDKs' recipe hook is removed (`Format::carrying_recipe`, `SaveCx.recipe`).
  - Every save from the editor carries `graph_info {name, hash}`. The defaults write it into three formats and say so with `stamped: true`: PNG as two `iTXt` chunks, WAV as `LIST INFO ICMT` (the audio engine's WAV too), SVG as a `<metadata>` element. Nothing reads it back.
  - The editor no longer says "without its recipe", in the status line or headless.
  - Menu help and hover text no longer promise a recipe.
  - `popout.get` now has `hasRecipe`, so a shell can offer Recipe only when the value has one.
  - Kept for lineage-tracking engines: `Display.recipe`, `viewer.recipe`, `FileOpener`, `recipe_graph`, `SaveReply.recipe`, and floor rows F56, F63, F64, F77.
- **8A.** The Python SDK's `register` shows paths without Windows' long-path prefix (`register.shown`, the same rule as Rust's `programs::shown`). This covers the uv-cache warning and refusal, an unstartable program, and the manifest's own path in every message. Paths used to run or open files keep the prefix. Tested with Windows-style strings on the Mac; the Windows VM is down, so it was not run there.

## The recipe inventory

Sent to the controller before anything outside the SDK was touched.

| Where | What | Now |
|---|---|---|
| `docs/interface.md` 15 | `SaveReply.recipe`, `SaveRequest.recipe_graph` ("the saved recipe holds that whole graph") | Kept, described as lineage-tracking engines' only; `graph_info` and `stamped` added |
| `docs/interface.md` 10.2, 7.6 rule 9 | `Display.recipe` (F56), lineage (F63) | Kept; only lineage-tracking engines send one, a viewer offers Recipe only then |
| `NodeFlags.recipe` | the input whose recipes a node gets (a pack flag) | Kept (the research software's) |
| `session file_cmds` `FileOpener` | an application's opener of a picture holding its recipe (F77); PlumCraft registers none | Kept |
| `coordinator` | fills `recipe_graph` across engines | Kept |
| `session viewer_cmds` `viewer.recipe` | shows and copies `Display.recipe` | Kept |
| `editor-core control.rs`, `headless.rs` | "without its recipe" | Removed: "with its recipe" only when a reply says so |
| `menu.rs`, `canvas_cmds.rs`, `popouts.rs`, `scene.rs`, `canvas.rs`, `save_cmds.rs`, `control-protocol.md` | wording promising a recipe | Fixed |
| egui `recipe_view.rs`, `watched.rs` | the Watched panel's Recipe section | Unchanged: already shown only when there is one |
| SDKs (`save.rs`, `save.py`) | a carrying-recipe hook no default used | Removed |
| Engines (text, array, audio, ports, craft) | none embedded or filled a recipe | Unchanged |
| Conformance cases | none mention recipes | Unchanged |
| `native/` | Mac and iPad `RecipeButton` (`Viewers.swift`), Mac person check "the pop-out's Recipe", Windows `Popouts.cs` Recipe flyout and "its recipe included" tooltip | For the shell agents, below |

The handle `recipe` (`md5:nd1:`, `/v1/blobs/{algo}/{recipe}/…`) is the digest recipe, a different thing, and is untouched.

## The name and graph hash

- **Name:** the document's title: its file's name without the extension, or `Untitled`.
- **Hash:** `sha256:` plus the SHA-256 of the open graph's JSON exactly as the editor writes it (`plumcraft_session::trust::graph_hash`, the hash the trust file already keeps). It covers every node with its kind, values, position and title, plus the wires, groups, frames and nets. So moving a node changes it, and two layouts of the same graph differ. A result-only hash was offered and not answered; that is open.
- **Where it goes:**
  - PNG: `iTXt` chunks `PlumCraft graph` and `PlumCraft graph hash`.
  - WAV: one `ICMT` reading `PlumCraft graph "<name>", <hash>`.
  - SVG: `<metadata>` with the same text.
  - JPEG, TXT, CSV, JSON, NPY, FLAC and AIFF: nothing, so their data stays as it was.
- **Compatibility:** `graph_info` and `stamped` are additive, so old engines and editors ignore them. `recipe` is unchanged, so no capability or version is needed.
- **How a person sees it:** in the file itself (any PNG or WAV metadata viewer).

  Proposed, not built: a pop-out's Info shows the name and hash of a value's last save. The Recipe button stays for lineage-tracking engines.

## 6A: questioning a non-picture shape (proposed, not built)

PNG stays first for ARRAY (6A).

**The rule.** A shape is a picture when it is 2-D or 3-D, not a table, with at most 4 channels, its smaller side at least 64, and its longer side at most 16 times its shorter.

Why 16:1: panoramas and banners rarely pass 8:1, and 16:1 leaves room for them. A 64 × 4096 strip, such as a waveform drawn as rows, is 64:1 and is questioned.

**Why it isn't built.** Only the engine knows the value's shape at save time. The core has it for watched and popped-out values, but not for a result node's footer. So the engine must answer a person's save with a question the editor can recognise. Today a reply carries only `ok`, `error`, kind and message, and parsing messages would be fragile. That needs a message addition, so I stopped there.

**The proposal:**
1. The engine lists a new capability, `output.save.question`.
2. The editor sends `ask: true` with saves a person started. A scripted save, or one with an explicit format from a script, never sends it.
3. For a questioned shape, the engine replies `{ok: false, question: {text, shape, offer: "npy"}}` and writes nothing.
4. The core raises a prompt naming the shape, with three answers: "Save as PNG anyway", "Save as NumPy (.npy)", "Cancel".
5. It resends with `ask: false`, or with `format: "npy"` and the path's extension changed.
6. An engine without the capability ignores `ask` and saves as today.

This needs a new `Prompt` kind that the shells draw. The Apple and Windows shells already draw prompts from `{kind, title, text, answers}`; whether they take an unknown kind generically is theirs to confirm.

## The preferences file in the real Library (B124)

- **Not this workspace's Rust or Python tests.** Each suite was run with `HOME` at a fresh scratch folder (`claude-debug/sdk-save/home-probe.sh` and `home-probe-py.sh`): the whole Rust suite (1,639 tests) and the Python suite (247). Neither wrote preferences there.
- **No caller with that name.** The path is `prefs::path_for("PlumCraft")`, and no source here calls it with that name:
  - the egui app's name is `plumcraft-editor`;
  - the C ABI writes only to `dirs.prefs` or `PLUMCRAFT_EDITOR_PREFS`;
  - the Mac app's folder is `Plummer`, the iPad's `plummer`.
- **This agent was not running tests at 15:29:31.** Its gate was compiling for clippy; its test stage started at about 15:33.
- **Left as suspects, not run here:**
  - the Swift kit's tests (a `ShellConfig` with `appName: "PlumCraft"` and `dirs.prefs` in the real Library, from a test that never touches `testEnvironment`);
  - the iPad simulator's tests;
  - the Windows kit's `dotnet test` on the Mac.
- **A second leak, found by the probe.** Rust tests exported values into the real `Library/Caches/plumcraft/open`.
- **Fixed at source.** `plumcraft_scratch::testing()` is true for a cargo-built test binary or any process with `COMMONKIT_TEST_RUN` set. When it is, `prefs::path_for`, the trust file beside it, and `doc_cmds::default_root` go under the process's scratch root. The probe after the fix finds nothing under the scratch home. The real file was not touched.

## What the shells must change

These are for the shell agents; `native/` is untouched.

- **Mac and iPad** (`native/apple/PlumCraftFFI/Sources/PlumCraftShell/Viewers.swift`): `RecipeButton` is always on the pop-out bar. For a value without a recipe it opens on "No recipe: the engine sent none for this value." Offer it only when `popout.get`'s `hasRecipe` is true. The Mac person check's "the pop-out's Recipe" step (`person-check.py`) needs a value from a lineage-tracking engine, or must expect the button to be absent.
- **Windows** (`native/windows/src/PlumCraft.App/Popouts.cs`): the Recipe flyout is always on the bar and says "No recipe: …"; offer it only when `hasRecipe`. Change the Save As tooltip "Save the value to a file, its recipe included" to "Save the value to a file".
- **Every shell:** the save's status line no longer says "without its recipe", and the menus' help texts changed. Nothing else is new to draw: contact sheets reuse the existing Contact Sheet… and file ask.

## Tests

The gate, run on the Mac:

- fmt clean; clippy (`--all-targets --all-features -D warnings`) clean.
- nextest **1,640 of 1,641**. The one failure is the known `params_coverage hidden_params_match_the_baseline`.
- Hash gates and the core-toolkit gate: ok.
- Schema regenerated (`graph_info`, `GraphInfo`, `stamped`).
- pytest **247 passed** (242 before), and ruff clean after a long line was split.
- Private strings: no match in the 1,298 tracked files or the new files.

New and changed:

| Where | What it checks |
|---|---|
| `crates/engine/src/save.rs` | the sheet's layout and pixels and the shared fixture; refusals; the name and hash in PNG (CRC checked), WAV and SVG; text untouched |
| `crates/engine/tests/save.rs` | a list of pictures saved as one PNG sheet over `output.save` (items, labels, `stamped`), JPEG sheets, npy refusing, text unstamped |
| `python/tests/test_save.py` | the same sheet pixels as the fixture; the sheet over the socket binding; the PNG, WAV and SVG bytes as Rust's |
| `python/tests/test_register.py` | 2 new: the long-path prefix taken off, in the rule's string cases and in register's printed warning and refusal |
| `crates/session/src/save_cmds.rs` | `graph_info`'s name and hash in the request |
| `crates/ffi/tests/dirs.rs` | `popout.get`'s `hasRecipe` false for the text engine |
| `crates/ui/tests/kittest_save.rs` | no word of a recipe after a sheet saved without one |

## Unverified

- No launched app.
- No Linux run.
- No Windows run: 8A was tested with strings on the Mac only.
- The shells' Recipe buttons are unchanged.
- No viewer shows the name and hash.

## Decisions for the maintainer

1. **The graph hash:** the whole document's, positions included (built), or a hash of only what makes the result (kinds, values and wires)?
2. **6A's question:** build the proposal (a capability `output.save.question`, `ask` on a person's save, a `question` reply, a new prompt), or refuse non-picture shapes with `config` instead?
3. **The aspect limit for 6A:** 16:1, or another ratio?
4. **Labels on contact sheets:** not drawn today, because text rendering differs between the SDKs and fonts. Draw them in Rust only (pixels would then differ between the SDKs), or keep labels in the reply only?
5. **Where a person sees the name and hash:** an Info line in the pop-out for a saved value, or only in the files?
