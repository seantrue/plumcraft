# N1 slice 1: the editor core crate, the pure logic, and the pump

Branch `native/n1-core-crate`, working tree only (nothing committed, staged or stashed). 2026-11-19.

## Summary

- New crate `crates/editor-core` (`plumcraft-editor-core`, 0.7.0, MIT OR Apache-2.0, the crate-level clippy denies, workspace lints, licence files, a `[workspace.dependencies]` entry like its siblings). It depends only on `plumcraft-host`, `-proto`, `-session`, `-store`, `serde`, `serde_json`.
- Toolkit-free, proved by `cargo tree` and held by a new gate, `scripts/check-core-toolkit-free.sh`, wired into CI's Linux `gate` job beside the hash gates (no other CI change).
- Moved: prefs, appearance, run animation, JSON-as-YAML, theme tokens as data, probe, diff, dtype, the raster type, bent-wire geometry, the palette index, run texts, and the event pump (`Link`), now woken by a `Waker`.
- `plumcraft-ui` re-exports everything at the old paths. The research software's downstream app compiles unchanged against this tree (`cargo check --tests`, exit 0).
- `headless.rs` and `plumcraft-ffi` now use the core's `Link`.
- Tests: the same single expected failure before and after, under both hashes. 1,297 tests ran before and 1,306 after: 63 moved and 9 are new.

## What moved where

| Old path | New path | What stayed behind, and why |
|---|---|---|
| `crates/ui/src/prefs.rs` | `crates/editor-core/src/prefs.rs` | Nothing. `plumcraft_ui::prefs` is `pub use plumcraft_editor_core::prefs` |
| `crates/ui/src/appearance.rs` | `crates/editor-core/src/appearance.rs` | Nothing. `resolve` now returns the core's `Tokens`; the two ui call sites wrap it in `Tokens::from` |
| `crates/ui/src/run_anim.rs` | `crates/editor-core/src/run_anim.rs` | Nothing |
| `crates/ui/src/viewers/yaml.rs` | `crates/editor-core/src/yaml.rs` | Nothing. `viewers::yaml` is a re-export |
| `crates/ui/src/theme.rs` (tokens, values, logic) | `crates/editor-core/src/theme.rs` | In ui, the egui-coloured `Tokens` with `Color32` fields (kept for API), `FamilyTint`, `CLEAR`, the conversions, `Tokens::apply`, `font`, `bold_family`, the `token`/`token_mut` table (field access on the egui struct; a parity test holds it to the core's), and thin wrappers for `mix`, `parse_hex`, `hex`, `luminance`, `contrast`, `oklab`, `oklch`, `from_oklch` and `delta_e` |
| `crates/ui/src/probe.rs` | `crates/editor-core/src/probe.rs` | In ui, a glob re-export plus egui-typed wrappers for `display_to_array` and `array_to_display` |
| `crates/ui/src/diff.rs` | `crates/editor-core/src/diff.rs` | In ui, a re-export plus a `diff()` wrapper that takes the NaN colour as `Color32` |
| `crates/ui/src/viewers/grid.rs` `Dtype` | `crates/editor-core/src/dtype.rs` | The grid viewer (egui). `Dtype` is re-exported there; the probe needs it |
| `crates/ui/src/media.rs` `Rgba` (raster) | `crates/editor-core/src/raster.rs` | `image()` becomes the extension trait `media::RgbaImage`, since it makes an `egui::ColorImage`. Decoding and `shrink` stay for the media-crate slice |
| `crates/ui/src/wire_route.rs` `smooth_path`, `path_distance`, `insert_index`, `STEPS` | `crates/editor-core/src/wire_path.rs` (plus `as_point`) | In ui, egui-typed wrappers with the old signatures, plus `Routed`, `RouteDrag`, `wire_color`, the Canvas methods, the drawing and the handles (all egui-snarl pin ids or painting) |
| `crates/ui/src/palette.rs` `PaletteIndex`, `display_name`, `category_path`, `MAX_SHOWN` | `crates/editor-core/src/palette.rs` | `show`, `node_menus`, `PaletteState` (egui `Pos2` and a focus flag: UI state), `PaletteOutcome`, `SEARCH_HINT`. `PaletteIndex::category` became `pub` so the ui's `show` can call it |
| `crates/ui/src/run_view.rs` `ERROR_CHARS`, `EXCERPT_*`, `NOT_RUN*`, `state_name`, `shows_stale`, `running_text`, `is_running`, `cached_text`, `format_ms`, `truncate_chars`, `excerpt_lines` | `crates/editor-core/src/run_text.rs` | `THUMB_PX`, `THUMB_HOVER_PX`, `MAX_THUMB_SIDE`, `thumb_size` (returns `egui::Vec2`), thumbnails, and snarl's wire geometry (`wire_frame`, `wire_points`, `wire_distance`, `wire_midpoint`: degree-5 snarl curves, for N2) |
| `crates/ui/src/canvas.rs` `stale_tooltip`, `OLDER_THUMB` | `crates/editor-core/src/run_text.rs` | Re-exported in `canvas` |
| `crates/ui/src/link.rs` `Link` | `crates/editor-core/src/link.rs` (`Link`, `Waker`, `MAX_WAITING`, `state_text`) | In ui, a newtype `Link(pub core::Link)` with the old `start(conn, label, Option<egui::Context>)`, `Deref`/`DerefMut` to the core's, `From<core::Link>`, `into_core`, and `repaint_waker(ctx)` |

New in ui: `crates/ui/src/plain.rs`, the conversions between the core's plain types and egui's.

## The plain types

All are in `plumcraft_editor_core::plain`, `Copy`, and numbers only.

| Type | Holds | Notes |
|---|---|---|
| `Color` | `r, g, b, a: u8`, **premultiplied** like `Color32` | `from_rgb`, `from_gray`, `from_rgba_premultiplied`, `from_rgba_unmultiplied` (egui's fixed-point `mul_frac_round`, copied exactly), `to_array`, `TRANSPARENT`, `BLACK` |
| `Point` | `x, y: f32` | Both a point and a step (egui's `Pos2` and `Vec2` in one): `+ - * /`, `length` (`hypot`, as emath), `length_sq`, `dot`, `distance`, `ZERO` |
| `Rect` | `min, max: Point` | `from_min_max`, `from_min_size`, `width`, `height`, `center`, `contains` (egui's inclusive edges) |

- **Conversions are traits, not `From`.** `ToEgui` and `ToCore` live in `plumcraft_ui::plain`. Rust's orphan rule forbids `impl From<egui::Pos2> for core::Point` in `crates/ui`, since both types are foreign there, and the core may not name egui.
- **`From` was possible where the ui owns the type:** `From<core::Tokens> for ui::Tokens`, `From<&ui::Tokens> for core::Tokens`, and `From` both ways for `FamilyTint`.
- **Bit-exactness is tested in ui.** Every alpha and a sweep of channels give the same bytes from `Color::from_rgba_unmultiplied` as from `Color32::from_rgba_unmultiplied`. Distances, dots, scaled sums, rect centres and `contains` give identical `f32` bits to emath's.
- **Core theme additions:** `theme::WidgetPalette` (data twin of craft_ui's), `theme::ramp(stops, none, t)` (a free function), `Tokens::family_tint_pinned(pin, catalog, family)`, `Tokens::size` (now `pub`), and `Tokens::text_size_of(Text)`, which replaces `font()` in the core.

## The `Waker`

`pub struct Waker(Arc<dyn Fn() + Send + Sync>)`, with `Waker::new(closure)`, `wake()`, `Clone` and an opaque `Debug`. `Link::start(conn, label, Option<Waker>)`.

Why a shared closure rather than a trait:

- Every shell already has a one-line wake: egui's `request_repaint`, a C function pointer with its context, a channel send. A closure adapts each with nothing to implement.
- The `Arc` makes it cheap to give to several pumps.
- It matches the sketch in `docs/shell-interface.md` 5.1 (`Arc<dyn Fn() + Send + Sync>`).
- It carries no payload, so nothing crosses on the pump's thread but the call (plan 4.2).

A named newtype rather than a bare `Arc` gives the type a place for docs and `Debug`.

The egui shell builds one with `plumcraft_ui::link::repaint_waker(ctx)`. The FFI's sends `Msg::Drain` to its core thread.

## Public API: re-exports, and what a downstream app would notice

- **Old paths still work:** `plumcraft_ui::{prefs, appearance, run_anim}` are module re-exports, as `layout` already was.
- **Re-exported items:**
  - `viewers::yaml` and `viewers::grid::Dtype`;
  - `media::Rgba`;
  - `probe::*`, `diff::*`, `palette::{PaletteIndex, …}`, `run_view::{format_ms, …}` and `canvas::{stale_tooltip, OLDER_THUMB}`;
  - `link::{MAX_WAITING, Waker, state_text}`;
  - `theme::{BASE_TEXT, Check, FRUITS, OLIVE, PLUM, TEXT_SIZES, TOKENS, Text, ZOOMS, fnv1a, fruit_index, fruit_slots}`.
- **Unchanged signatures:** `theme::Tokens` keeps its `Color32` fields, its methods and their signatures, and the consts `PAPER`, `PEWTER`, `DUSK`, `DEFAULT` and `ALL`. `link::Link::start(conn, label, Option<egui::Context>)` and `engine_view::Connector` are unchanged.
- **What a downstream app would notice:**
  1. `Appearance::resolve` returns `plumcraft_editor_core::theme::Tokens`. Wrap it: `plumcraft_ui::theme::Tokens::from(a.resolve(name))`.
  2. `media::Rgba::image()` is a trait method: `use plumcraft_ui::media::RgbaImage`.
  3. `plumcraft_ui::link::Link` is a tuple newtype around the core's. Field and method access work through `Deref`; anything naming its private fields never compiled anyway.
  4. `theme::Text` and `theme::Check` are the core's types, with the same variants and fields.
- **Checked on the research software** (the private downstream; it uses `link::Link`, `theme::Tokens` fields, `value_editors`, `engine_view::Connector`, `run_view::rgba_image`, `offscreen::render`, `viewers::viewable`). A copy of its sources in `claude-debug/native-n1/<app>-check/` was built with a scratch `.cargo/config.toml` patching every `plumcraft-*` crate to this tree. `cargo check --tests` exits 0, with no source change. Its repository was not touched.

## `cargo tree` evidence

- **`plumcraft-editor-core`:** `cargo tree -p plumcraft-editor-core -e normal --depth 1` shows `plumcraft-host`, `plumcraft-proto`, `plumcraft-session`, `plumcraft-store`, `serde` and `serde_json`.
- **The gate** (`--all-features --target all -e normal,build`, 204 lines) finds none of egui, egui-snarl, egui_extras, egui_plot, egui_kittest, eframe, epaint, winit, wgpu, rfd or plumcraft-ui. `scripts/check-core-toolkit-free.sh` prints "core gate: ok". The same pattern run on `-p plumcraft-ui` does flag eframe, egui, egui-snarl and the rest, so the gate catches what it should.
- **`apps/plumcraft-cli` (package `plumcraft`):** no toolkit, before or after (it never depended on the ui).
- **`plumcraft-ffi`:** no toolkit (now depends on the core).
- **`apps/plumcraft-editor` (holds `headless.rs`):** `headless.rs` no longer names `plumcraft_ui` and links engines with `Launcher::connect_core`, which returns the core's `Link`. But the crate itself depends on `plumcraft-ui`, eframe, egui, winit, wgpu (through eframe) and rfd (through the ui). So `plumcraft-editor run`, `check`, `tidy` and `nodes` still build and link egui.
  - Fixing it means moving `Launcher`, `EngineKind`, `Mode`, `External` and the connect functions out of the windowed `lib.rs` into a windowless crate or feature, with headless beside them. That is more than a small change, so it is left (plan 4.4's "small change" row is not met yet).

## Tests

All runs used the CI command with `COMMONKIT_STORE_MIN_FREE_GB=2` and a fresh run folder under `claude-debug/native-n1/`; logs are beside them.

| Run | Ran | Passed | Failed | Skipped | Failing test |
|---|---|---|---|---|---|
| Baseline, b3 | 1,297 | 1,296 | 1 | 9 | `plumcraft-craft::params_coverage hidden_params_match_the_baseline` |
| Baseline, md5 | 1,297 | 1,296 | 1 | 9 | the same |
| After, b3 | 1,306 | 1,305 | 1 | 9 | the same |
| After, md5 | 1,306 | 1,305 | 1 | 9 | the same |

- **The one failure is the expected one.** Its stderr lists effectcraft and photocraft curated-node catalogue checks against the craft apps installed on this Mac. No foreign-digest or bad-key marks in any run.
- **Test count:**

  | Crate | Before | After |
  |---|---|---|
  | `plumcraft-ui` | 395 | 337 |
  | `plumcraft-editor-core` | none | 67 |

  - 63 unit tests moved with their modules. Ten changed module path (`run_view::tests::{durations, long_errors_and_excerpts_are_cut, states_have_words}` became `run_text::…`; `wire_route::tests::*` became `wire_path::…`; `viewers::yaml::tests::*` became `yaml::…`). Every moved test is present and passing.
  - New in the core (4): `link::events_are_held_until_drained_and_wake_the_shell`, two `plain` tests, and the core's `theme::text_sizes_follow_the_base` (the core's own sizes).
  - New in ui (5): two `plain` parity tests and three `theme` parity tests (themes, token table, methods).
  - `plumcraft-ffi` runs 9 tests and `plumcraft-editor` 6, before and after.
- **No test file changed.** Nothing in `crates/*/tests`, `apps/*/tests` or any test module outside the moved files was edited.
- **Gates:**
  - `cargo fmt --all --check`: clean. `cargo clippy --all-targets --all-features -- -D warnings`: clean.
  - `scripts/check-hash-gates.sh`: "hash gates: ok". It prints `awk: can't open file` for the four deleted ui files, because `git ls-files` still lists unstaged deletions. That goes away once the deletions are staged.
  - The hash gate does not see untracked files, so I ran its Rust pattern by hand over `crates/editor-core/src/*.rs` and `crates/ui/src/plain.rs`: no hits.
  - `scripts/check-core-toolkit-free.sh`: ok.
- **Python:** untouched. `uv run ruff check .` passes. A bare `uv run pytest` fails `test_one_algo.py::test_hash_is_what_was_asked`, which requires `COMMONKIT_HASH` to be set. Run as CI runs it, it gives 224 passed under b3 and under md5.

## Chosen not to move, and why

- **egui-touching theme code:** `Tokens::apply`, `font`, `bold_family`, `CLEAR` as `Color32`, `craft_ui::theme::WidgetPalette::apply_to`. The core has `text_size_of`, and the shell picks the face.
- **`run_view` thumbnails and snarl wire geometry** (`Thumbs`, `load_picture`, `wire_frame`, `wire_points`, `wire_distance`, `wire_midpoint`), and `thumb_size` (returns `Vec2`). The wire functions are snarl's degree-5 curves, which N2 replaces with the fitted cubic chain (shell-interface 15.3).
- **`wire_route::wire_color`:** a one-line lookup returning `Color32`, only used for drawing. It could follow easily.
- **`palette::show`, `node_menus`, `PaletteState`:** drawing, plus UI state (`screen_pos`, `graph_pos`, `focus_requested`) that belongs to N2's view model.
- **`media::shrink` and every decoder:** the `plumcraft-media` slice.
- **`playback.rs`:** cpal, as instructed.
- **`status_msgs`, `app_hooks`, `watched::Fulls`:** listed in plan 4.1 but not in this slice's brief.

## For later slices

- **Two `Waker`s.** `crates/ffi/src/core.rs` has its own `struct Waker` (the shell's C wake, coalesced with a pending flag), now beside the core's `link::Waker`, imported as `link::Waker`. When the FFI's core thread becomes the editor core's, these should become one.
- **The FFI pump changed slightly.**
  - It now holds at most `MAX_WAITING` (20,000) events between drains, as the editor always did; the old FFI pump forwarded every batch over an unbounded channel. The core thread drains on every wake, so this only matters for a stalled core thread.
  - Its thread is now named `plumcraft-editor-pump` (was `plumcraft-ffi-pump`; nothing referred to the name).
- **A duplicated token table.** The name-to-field table is written twice: in the core, and in ui as a macro over the `Color32` fields, because `token_mut` must return `&mut Color32`. `theme::tests::the_token_table_is_the_cores` fails if they drift. It goes when ui stops keeping its own colour struct, for example once egui draws from the scene in N2.
- **Theme cost per call.** Each ui `Tokens` method converts to the core's (about 50 `Copy` fields, no allocation) per call.
  - `family_tint` was already the costly one: `fruit_slots` builds a `BTreeSet` over the catalogue's families for every node on every frame. The scene in N2 should cache fruit slots per catalogue digest.
  - `ramp` converts only its six stops, since it runs per spectrogram pixel.
- **A const-context workaround.** A `const` can't drop a `BTreeMap`, so the ui theme consts are built by a private `const fn from_theme(core::Tokens)` that `mem::forget`s its argument, whose pins must be empty. This is documented at the function, and the non-const paths take the pins out first.
- **Toolkit facts in the core's data.** `Tokens.semibold` (is a SemiBold face installed?) is a shell fact, and `Appearance.zoom` is egui's zoom factor. Native shells will read them as "has a bold face" and "UI scale".
- **No new global state.** None of the moved modules has process-wide state: `prefs::Store` is per editor, and the themes are consts. The 4.3 audit is unaffected by this slice.
- **Preferences are written on the UI thread.** `prefs::Store` saves synchronously (`save()` on every `remember`/`set_*`), which is file I/O on the UI thread today. It moved as is; plan 4.2's threading should take it.
- **Not commands yet:** palette open and filter state, and the wire bend-point drag (`RouteDrag`), are egui-local state (shell-interface 15.3; N2).

## Lines of code

From `git diff --numstat`, plus untracked files.

| Area | Added | Removed |
|---|---|---|
| `crates/ui/src` (moved out, adapters in) | 384 | 3,303 |
| `crates/ui/src/plain.rs` (new, untracked) | 114 | 0 |
| `crates/editor-core` (new, untracked: 4,040 lines of `src`, 25 of `Cargo.toml`, plus licence files) | 4,065 | 0 |
| `crates/ffi` | 29 | 37 |
| `apps/plumcraft-editor` | 28 | 10 |
| `Cargo.toml`, `Cargo.lock`, three crate manifests | 24 | 0 |
| CI (`ci.yml`) and `scripts/check-core-toolkit-free.sh` (new) | 19 | 0 |
| `docs/guide.md` | 1 | 0 |

The biggest single file is `crates/ui/src/theme.rs` (+289 −675, now 655 lines). The rest of the ui's added lines are one-line re-exports and small wrappers. Of the core's 4,040 lines, about 3,300 moved with only type renames; the new code is `plain.rs` (181), the `Waker` and the `Link` test (~90), `run_text.rs`'s header, `lib.rs`, and the theme's `WidgetPalette`, `ramp`, `family_tint_pinned` and `text_size_of`.

## Release tooling

- `cargo publish --workspace` picks the crate up. It has a `[workspace.dependencies]` entry with `version = "=0.7.0"`, so `plumcraft-ui`, `plumcraft-editor` and `plumcraft-ffi` can depend on it publishably.
- `cargo package -p plumcraft-editor-core --list` shows its sources and both licence files.
- Nothing in `packaging/` lists crates by name, so no edit there.
- I added a row for it to `docs/guide.md`'s crate table.

## What the docs should say

**`docs/STATUS.md`:** a line under the native editors item, for example:

> N1 slice 1 (branch `native/n1-core-crate`): `plumcraft-editor-core` cut from `plumcraft-ui` with no user-visible change. It holds prefs, appearance, themes as data (plain `Color`), run animation, run texts, bent-wire geometry, the palette index, probe, diff, dtype, raster, YAML, and the event pump `Link` with a `Waker`. The ui re-exports every moved item at its old path, and the research software compiles unchanged. `headless.rs` and `plumcraft-ffi` use the core's `Link`. `scripts/check-core-toolkit-free.sh` gates the core in CI. 1,306 Rust tests: the one expected failure (`params_coverage`, installed craft apps) under both hashes.

**`docs/research/24-native-shells.md`:**

- 2.1: the "windowless use pulls in egui" bullet should say `headless.rs` now uses the core, but `apps/plumcraft-editor` as a crate still links egui.
- 4.4, the `apps/plumcraft-editor` row: the split of `Launcher`, `EngineKind` and `Mode` into a windowless crate or feature is still to do.
- 4.1: mark `ui/link.rs`, `prefs.rs`, `appearance.rs`, the theme tokens, `run_anim.rs`, `palette.rs` (index), `probe.rs`, `diff.rs` and `viewers/yaml.rs` as moved in N1 slice 1. Note that `diff.rs` landed in the core, not yet in `plumcraft-media`.
- Section 3, rule 1: record that the plain types are `Color` (premultiplied, as egui's), `Point` and `Rect`, and that a shell converts through its own traits because of the orphan rule.

**`docs/shell-interface.md`:**

- 5.1: the `Waker` is `plumcraft_editor_core::link::Waker`, a newtype over `Arc<dyn Fn() + Send + Sync>`. `Link::start(conn, label, Option<Waker>)`.
- Appendix A: `prefs.rs`, `appearance.rs`, `run_anim.rs` and `viewers/yaml.rs` now live in `crates/editor-core`. Of `theme.rs`, only the egui adapter is left. `link.rs` is the egui newtype. `probe.rs` and `diff.rs` are adapters, and `diff.rs` is in the core rather than `plumcraft-media` for now. `palette.rs` is drawing only. `run_view.rs`'s formatters and `canvas.rs`'s `stale_tooltip` are in the core's `run_text`.
- 14.1 (`theme.get`) can now be served from `plumcraft_editor_core::theme::Tokens`.

**`AI.md`:** at commit time, a ledger entry for this slice, the contributions as above. No defects are known from this slice.
