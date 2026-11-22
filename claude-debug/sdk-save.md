# Save Result as a basic feature of the engine SDKs

*2026-11-22, branch `feat/sdk-save` from `origin/feat/native-n0` (13c903b), uncommitted.*

The maintainer, asked "F19: add save formats to the Rust SDK so a result node can offer Save Result, or leave the row unmet?": "Make the save an basic feature." Both SDKs now declare save formats and save a node's result, with defaults for every core type, so an engine whose nodes output core types gets Save Result with no code. The editor's Save Result… was broken outside egui and never wrote a remote engine's files; both are fixed in the core.

## What an engine author writes

Nothing, for the defaults. A node that outputs a core type offers the SDK's formats for that type:

| Type | Formats (the first is the default) |
|---|---|
| TEXT | `txt` |
| TABLE | `csv`, `json` |
| JSON | `json` |
| IMAGE | `png`, `jpeg` (option `quality`, 1 to 100, 90) |
| ARRAY | `png` (its pixels when 8-bit with 1 to 4 channels, else the viewer's `auto` picture), `jpeg`, `npy` |
| SVG | `svg` |
| AUDIO | `wav` (option `bits`: `float32` or `int16`), `npy` |

A format of its own, its options asked for by the editor's form:

```rust
let shout = Format::new("shout", "Shouted Text", &["shout"], &[Media::Text], "text/plain", |v, cx| Ok(encode(v, cx.options)))
    .with_option(Port { choices: Some(vec![json!("!"), json!("?")]), ..option("mark", "Mark", "ENUM", json!("!")) });
let engine = engine.with_save_format(shout);       // or replace a default by its id
def.save_formats = Some(vec!["shout".into(), "txt".into()]); // a node's own list, in order
```

```python
class Loud(Engine):
    id, save_formats = "loud", (save.Format("shout", "Shouted Text", ("shout",), ("text",), "text/plain", encode).with_option(mark),)
    @node("Shout", save_formats=["shout", "txt"])
    def shout(self, text: str = "plum") -> Text: return text
```

`without_save_format(id)` and `without_default_save_formats()` take formats away; `output.save` is listed while the engine has any. The SDK runs the graph for the node (a memo hit after a run), picks the format (by id, else the path's extension, else the node's first), checks and defaults the options, encodes, and writes the file whole to `path`, or returns it as a blob when there is no `path`. A list saves as numbered files (`<stem>-00.<ext>`), one item with `index`. A failure is data.

## What was missing on the editor side

1. **Save Result… did nothing in every shell but egui.** The result node's menu item and footer button are `run.save {node, port, sheet, ask: true}`; only egui intercepted `ask: true` and asked where. The core's `invoke` ran `run.save` without a path, which failed. Now `invoke_with` asks `file.save` with the output's formats, then the chosen format's options with a `form` ask when it has any, then saves (golden rule 4). egui still calls the same function.
2. **Options were never asked for.** They are now, through the core's form (`save_options_form`, `save_options_read`), refused values keeping the form open with the reason.
3. **Formats were offered by the catalogue, not the output.** The menu, footer and dialog offered every format of every engine whenever the catalogue had one. They now offer `save_formats_for(def, port)`: the node's own list, or the formats whose media take the output's. A Convert's value offers its last step's.
4. **A remote engine's files were never written.** With `data.path` off the HTTP host strips `path` and the engine returns `files`; the reply was taken as saved and nothing reached the disk. `editor-core/src/saved.rs` now fetches each file by its ref and writes it whole where the person chose (several beside it, numbered from the chosen name; the engine's names are never used as paths).
5. **egui could not draw a form the core builds.** Its form window took only the static command forms; it now draws any `form` ask from its params.

## Interface changes (all additive, no capability)

- `NodeDef.save_formats` (section 7.3): optional list of format ids, in order; absent, every format whose media takes the output's; `[]`, none. Optional like `editor`, so no capability: an editor that ignores it offers every format.
- Section 7.1 `save_formats`: an engine that lists one has `output.save`; ids are shared across engines in a merged catalogue; the SDKs' defaults listed.
- Section 13.2 step 8: the editor writes returned files where the person chose, numbered from the chosen name; a file's `name` is never a path.
- Section 15 `output.save`: format choice, options, where the file goes, numbered lists, `index`, `sheet`, `recipe`, errors (`config`, the node's own, `cancelled`, `op`, `engine`), that a save has no cancel op, and the bindings.
- Section 16.1: an engine on the SDK's executor saves with its formats.
- `docs/control-protocol.md`: the save's form ask and `run.save` with `ask: true`.
- `plumcraft-proto`: `SchemaReply::save_formats_for`, `SchemaReply::media_of`; the schema regenerated.

## Tests

The gate, run here on the Mac: `cargo fmt --all --check` clean; clippy `--all-targets --all-features -D warnings` clean; `cargo nextest run --all-features --no-fail-fast` **1,634 of 1,635** (the one failure the known `plumcraft-craft::params_coverage hidden_params_match_the_baseline`; baseline was 1,609 of 1,610); `scripts/check-hash-gates.sh` ok; `scripts/check-core-toolkit-free.sh` ok; schema regenerated (`protocol2.json`, `NodeDef.save_formats`); `(cd python && uv run pytest && uv run ruff check .)` **242 passed** (baseline 235; 7 new) and ruff clean; private strings: no match in the tracked files, and none in the new files (`claude-debug/sdk-save/private-new.sh`).

New and changed tests:

| Where | Tests |
|---|---|
| `crates/engine/src/save.rs` | 7 unit: npy headers, CSV quoting, WAV bytes and 16-bit rounding, options checked and defaulted, numbered names, defaults by type with add/replace/remove, PNG and JPEG from arrays |
| `crates/engine/tests/save.rs` | 7: the catalogue and `output.save` cap; writing to `path` (txt, shout with option, csv, png, npy, no temp files); lists numbered or one item, `sheet` refused; files returned as blobs without `path`; refusals as `config` and the node's own error; a save after a run is a memo hit; a hostile sweep of every field |
| `crates/engine/src/host.rs` | 1 renamed: an executor engine refuses what it can't save with `config` |
| `crates/audio/tests/conformance.rs` | `save_result_as_wav_and_flac` in mode 1, 2 and 3 (3 tests): SoundCraft's WAV (16-bit) and FLAC decode to the tone; FLAC refuses float |
| `crates/ports/src/lib.rs` | 1 new (both formats' bytes through the host) and 1 extended |
| `crates/proto/tests/hostile.rs` | 1 new (`a_nodes_save_formats_come_from_the_catalogue`); two new seeds, and `save_formats_for` run on every hostile catalogue |
| `crates/session/src/save_cmds.rs` | 1 new: each output offers its own formats |
| `crates/editor-core/src/saved.rs` | 1: returned files written where chosen, never by the engine's names |
| `crates/editor-core/src/invoke.rs` | 1: the options form's fields and how answers are read |
| `crates/editor-core/src/conformance.rs` | `the_ports_cases_pass_on_the_cores_own_document` (all four ports cases, the save's among them) |
| `crates/ffi/tests/conformance.rs` | `save_result_passes_through_the_c_abi` (the test engine over HTTP, from an engines folder) |
| `crates/ui/tests/conformance.rs` | `the_ports_cases_pass_on_the_egui_editor` now runs the save's case (`saves` claimed) |
| `crates/ui/tests/kittest_save.rs` | 11, 1 new (the options form refuses 12, sends 7) and the rest through the options form |
| `kittest_popout_bar`, `ffi/tests/dirs.rs`, `apps/plumcraft-editor/tests/headless.rs` | updated: the text engine now saves TEXT, so Save As asks and writes, and the headless run writes `.txt` |
| `python/tests/test_save.py` | 7, through the socket binding (hello, `output.save`): the catalogue and cap, writing to `path`, lists, files as blobs, refusals, the encoders' bytes against the Rust SDK's, pictures and defaults |
| `python/tests/golden/text_schema.json` | regenerated: the Python text engine's catalogue now lists its formats |

The conformance case "ports: Save Result writes a result node's value in each of its formats" (`needs: [engine:ports, saves]`) passes on the core's document, the C ABI and egui: the footer and menu offer Save Result…; `ui.invoke run.save {ask: true}` asks `file.save` with Quoted Text then Text as kinds and `Result.quoted` as the name; `plain.txt` saves straight away; `marked.quoted` asks for `mark` with the core's form; the engine's Read Text File reads `[plum]` and `'[plum]'` back.

## What the native shells and the launched checks still need

- **Nothing new to draw.** The Apple and Windows shells already present `file.save` (purpose `result`) and `form` asks; Save Result… now reaches them from the core. A shell whose form view only knows static command forms must draw a `form` ask from its params (`command: "run.save"`, `pick` and `line` fields).
- **The Mac's launched check:** its Swift runner (`plumcraft-conformance`) must give each case a `$scratch` folder and claim `saves`; until then it skips the new case. Then the person check should save the test engine's result node from its footer through the real save panel and the options sheet and open the file.

## Defects introduced and caught

Caught before the end, all by the tests named:

- The Python `save` called `engine.save_formats()`, which the class attribute of the same name shadows (a tuple is not callable): `test_save.py` failed; the method is `all_save_formats()`.
- The old egui-only Save Result path hid that the core's `invoke` couldn't take `ask: true`; kittest_save passed before and after only because egui intercepts it. Found reading the code, fixed in the core, covered by the conformance case on the core's document and the C ABI.
- Asking for options broke 8 kittest_save tests at first (the form waited unanswered); the tests now go through the form as a person would.
- A first version wrote several returned files under the engine's names when they looked safe; replaced by numbering from the chosen name, so remote and local saves name files alike and a reply can never pick a path.
- The text engine now saving broke four tests that asserted it couldn't (popout Save As, the C ABI's Save As into Documents, the headless run, and the Python text catalogue's golden); each updated to the new behaviour and checked to write the right bytes.

None known left.

## Unverified

- No launched app: the Mac and iPad apps, the Windows app and the person checks weren't run (`native/` untouched). Their Swift and C# runners skip the new case until they claim `saves`.
- No Linux run (yellowfin) of this branch.
- The C ABI case's HTTP engine passed the same-host proof here, so the engine wrote `path` itself; the returned-files route is covered by `saved.rs`'s unit test and the SDK tests without `path`, not end to end through an HTTP engine that fails the proof.
- egui's options form is driven by kittest (values set on the model, the button clicked), not by typing into its fields.
- The coordinator with several engines saving one engine's node is unchanged code; no new test drives a save across two engines.
- PNG of float arrays through the `auto` view and JPEG quality were checked for decoding, not against a picture.

## Files

Worktree: an agent worktree of the repository, branch `feat/sdk-save`.

New: `crates/engine/src/save.rs`, `crates/engine/tests/save.rs`, `crates/engine/README.md`, `crates/editor-core/src/saved.rs`, `python/src/plumcraft_engine/save.py`, `python/tests/test_save.py`, this report.

Changed: `crates/proto/src/catalogue.rs`, `crates/proto/tests/hostile.rs`, `crates/proto/schema/protocol2.json`, `crates/engine/src/{lib,exec,host}.rs`, `crates/engine/tests/pack_ops.rs`, `crates/audio/src/{lib,io}.rs`, `crates/audio/tests/conformance.rs`, `crates/ports/{Cargo.toml,src/lib.rs}`, `crates/session/src/save_cmds.rs`, `crates/editor-core/{Cargo.toml,src/{lib,invoke,menu,scene,document,headless,conformance}.rs,fixtures/conformance/cases.json}`, `crates/ui/src/{form_view,asks_view}.rs`, `crates/ui/tests/{conformance,kittest_save,kittest_popout_bar}.rs`, `crates/ffi/{Cargo.toml,tests/conformance.rs,tests/dirs.rs}`, `apps/plumcraft-editor/tests/headless.rs`, `python/src/plumcraft_engine/{__init__,declare,exec,host}.py`, `python/tests/golden/text_schema.json`, `python/README.md`, `docs/{interface,control-protocol,STATUS}.md`, `docs/research/24-native-shells.md` (F19's row and the decision table), `Cargo.lock`.

Scratch (untracked): `claude-debug/sdk-save/` (env, run and gate scripts, logs, the README example run, the golden regenerator).

## Decisions for the maintainer

1. **Contact sheets in the SDKs:** none of the defaults makes one, so Contact Sheet… on an SDK engine's list result is refused with `config`. Add a default sheet for pictures (IMAGE and picture ARRAY, labelled from the list's labels), or leave sheets to engines? (Recommended: add one for pictures, the research software's main use.)
2. **Recipes in saved files:** the defaults set `recipe: false`, and the editor says "saved without its recipe". Embed the workflow in PNG (a text chunk) and the others never, or leave recipes to engines? (Recommended: PNG only, once there is an opener that reads it back.)
3. **Cancelling a save:** there is no cancel op for `output.save`; it answers or times out at 300 s. Add one (a capability) or keep it? (Recommended: keep until a slow save is reported.)
4. **Format ids across engines:** a merged catalogue keeps the first engine's format of each id, so two engines' different `wav` options merge into one. Accept (ids mean one format everywhere, as the SDKs' defaults do), or carry formats per engine? (Recommended: accept, as documented.)
5. **ARRAY's default:** `png` comes first for ARRAY (a table or 1-D array is refused, naming `.npy`). Keep `png` first, or make `npy` the default? (Recommended: keep `png`.)
6. **The Mac's launched check:** give the Swift runner `$scratch` and `saves`, and add a person-check step that saves from the footer through the real panel and the options sheet. Who and when: the agents now in `native/`?
