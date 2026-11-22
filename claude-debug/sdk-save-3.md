# Save Result, third part: the result hash, the shape question, sheet labels, a recipe node

*2026-11-22, branch `feat/sdk-save-3` from `origin/feat/native-n0` (26a208e, after PRs #166 to #169), uncommitted.*

The maintainer: "Decisions: 1B; 2A; 3A; 4B; 5A; 6A; 7A". Recorded in note 24 with the cards, and with B99 asked but not yet answered. Decisions 5A (the name and hash shown only in the files) and 6A (old trial builds, done by the controller) needed no code.

## 1B: the hash covers only what makes the result

`graph_info.hash` is now `trust::result_hash`, defined in `docs/interface.md` section 15 precisely enough for another implementation.

**What it covers.** The result form is `{format: "plumcraft.result", v: 1, nodes: {id: {kind, values, engine?, convert?}}, wires: [{from, to, slice?}]}`.
- `values` are the widget values exactly as the file holds them: not coerced to their port types, and nothing derived.
- `engine` is present only when the node is pinned to an engine, since another engine may compute a different result.
- `convert` is present only on a Convert node that keeps its path.
- Wires are sorted by `to`, then `from`, comparing each node and port by its UTF-8 bytes.
- The form is written in the typed canonical form of section 14.1 and hashed with SHA-256.
- Node ids are in: the wires name them, and an editor keeps a node's id for its life.

**What it leaves out.** Positions, titles, serials, views, fields shown on the face, the persist choice, frames, groups and their promoted ports (a group changes no wire), nets and labels (a label is only a way of drawing a wire), bends, and `meta`.

**Tests.** Vectors are in `crates/proto/fixtures/result_hash.json`.
- `crates/session/src/trust.rs` checks them. It also checks that a moved, retitled, collapsed and regrouped graph hashes the same, and that a changed value or wire hashes differently.
- `python/tests/test_result_hash.py` recomputes the vectors from the interface text alone, as a second implementation, and agrees.

**The trust file's hash** (`trust::graph_hash`) stays as it is and shares no code with this one. It is a security digest of the whole document a person reviewed, titles included, so it must keep changing whenever the file changes.

## 2A and 3A: the save asks about a non-picture shape

- **The rule.** An array is not shaped like a picture when it is a table, is not (H, W) or (H, W, C), has more than 4 channels, has a shorter side under 64, or has a longer side more than 16 times its shorter. Both SDKs implement it as `picture_shape`. The SDKs' `png` and `jpeg` formats are the ones that ask (`questioning`).
- **The protocol.**
  - The new capability `output.save.question` is listed only by the engine.
  - `SaveRequest.ask` is set only on saves a person started.
  - `SaveReply.question` is `{text, shape, offer: "npy"}`, sent with `ok: false`. The reply also carries a `config` error with the same words, so an older editor still says why.
- **Who is asked.** The core's Save Result… chain sends `run.save {question: "person"|"script"}`, so the engine may ask. A script's `run.save` with its own path never sends it.
- **The prompt.** When the engine asks, the core raises a `saveShape` prompt. Its answers are `png`, `npy` and `cancel`, and its params now carry `labels`: "Save as PNG anyway", "Save as NumPy (.npy)", "Cancel". The shape goes in `detail`.
  - `png` saves again without asking.
  - `npy` saves to the same name with the `.npy` extension.
  - egui draws the prompt from those labels.
- **Interface and schema.** Updated; the hostile sweep has request and reply seeds for both new fields.
- **Tests.** Both SDKs test the rule's edges and the question over the protocol.
- **Conformance.** The case "ports: saving an array that isn't shaped like a picture as PNG asks first" covers `npy`, then `png` anyway, then a scripted save that is never asked. It passes on the core's document, the C ABI with the test engine over HTTP, and egui.

## 4B: labels drawn on the contact sheet

The maintainer said: "Just use the Pillow font. We will deal with issues before Plummer is released to public."

- **The font.** Labels are drawn in both SDKs with a 6 × 11 bitmap font covering printable ASCII; any other character draws as `?`. The glyphs were extracted from Pillow 13.0.0's built-in default bitmap font by the scratch script `make_sheet_font.py`, which is not tracked. The same `sheet_font.json` sits in both SDKs, and a test checks the two copies match. No system font is used.
- **The layout.** Cells are 64 to 256 pixels on a side. When any item has a label, each cell gets a 15-pixel band under it with the label centred in dark grey.
- **Tests.** The fixture is the labelled one, and Rust and Python both match its SHA-256. I rendered the fixture's sheet and looked at it: the labels are legible, and the `é` draws as `?`. The picture is in the scratchpad (`sheet.png`, not tracked).
- **Licence.** The font's upstream origin and licence are not verified. It is listed in THIRD-PARTY.md with exactly where it came from, and filed as B125, a release blocker: "the contact sheet's label font: verify its licence or replace it before a public release". Note 24 records all three answers about the font.
- **The Ubuntu font, as read before the plan was overruled.**
  - The Ubuntu Font Licence lets a rasterised table be redistributed if it carries Canonical's copyright notice (taken from the font's metadata) and the licence, under that licence alone.
  - Its condition 2 names a modified version in two opposite ways: renamed away from "Ubuntu" if substantially changed, "Ubuntu derivative X" if not. The text doesn't say which a one-size ASCII bitmap is, so I stopped there.

## 7A: a node that supplies a recipe

- **How a node supplies one.** In the Rust SDK, a lineage-tracking node puts its recipe among its outputs under `_recipe` (`exec::RECIPE_KEY`, not a port); every watched display of that node then carries `Display.recipe`. No other lineage flag is needed (`NodeFlags.recipe` is the research software's own).
- **The test engine's nodes.**
  - `ports.traced` sends `{style: "traced", params: {seed}}`.
  - `ports.strip` is a result node whose value is a 4 × 1000 array, a shape that is not a picture.
- **Conformance.** The case "ports: a value with a recipe offers Recipe, and one without doesn't" passes on all three drivers. In it, `popout.get` says `hasRecipe: true` for Traced and `viewer.recipe` returns its recipe; the Result node says `false` and `viewer.recipe` returns null.
- **What each shell's person check should then do:**
  - **Mac:** add a Traced node from the test engine (installed as today), pop out its text, check the Recipe button is there, press it, and read "traced" in the popover. Pop out the Result node's value and check there is no Recipe button.
  - **iPad:** the same, in its compact pop-out sheet. The iPad can't start the test engine (it starts no programs), so this needs it attached over HTTP, or the check stays Mac-only.
  - **Windows:** the same in `person-check.ps1`, with the Recipe flyout on Traced's pop-out and none on the Result node's.
  - **Every shell:** save Strip as PNG from its footer and answer the prompt with its buttons, which should read the core's `labels`.

## What the shells must change (`native/` untouched)

- Read a prompt's new `labels` for the button words. Today the Swift `ShellWords.buttonTitle` and the C# `Word` derive the words from each answer's name, so they would show "Png" and "Npy".
- Offer Recipe only when `popout.get` says `hasRecipe` (from part 2, still open).

## Tests

The gate, run on the Mac:

| Check | Result |
|---|---|
| fmt | clean |
| clippy (`--all-targets --all-features -D warnings`) | clean |
| nextest | 1,645 of 1,646; the one failure is the known `params_coverage hidden_params_match_the_baseline` |
| hash gates, core-toolkit gate | ok |
| schema | regenerated (`ask`, `question`, `SaveQuestion`) |
| pytest | 249 passed (247 before) |
| ruff | clean, after one long test line was split |
| private strings | no match in 1,302 tracked files or the 6 new ones |

New tests:

| Where | What |
|---|---|
| `crates/session/src/trust.rs` | the result hash against the vectors, and what leaves it unchanged |
| `python/tests/test_result_hash.py` | a second implementation from the interface text |
| `crates/engine/src/save.rs` | the sheet with labels against the fixture; the font is printable ASCII and identical in both SDKs |
| `crates/engine/tests/save.rs` | the question and the rule's edges; the sheet's labels over `output.save` |
| `python/tests/test_save.py` | the same in Python, over the socket binding |
| `cases.json` | 2 conformance cases (the question; Recipe present and absent), passing on the core's document, the C ABI and egui |

## Unverified

- No launched app, no Linux run, no Windows run.
- egui's `saveShape` modal was not clicked through as a person would: it was driven through the control channel only.
- The iPad's route to the test engine (HTTP) is untried.

## Decisions for the maintainer

1. Draw the PlumCraft name and hash on contact sheets too, or only in the file (5A says files only; this asks whether the sheet's labels should show them)?
2. B99 is still open.
3. B125: verify the label font's licence, or replace it, before a public release.
