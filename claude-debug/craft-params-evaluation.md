# Craft commands and the parameters the packs expose

2026-11-04. Probe: `claude-debug/craft-params-probe/`, untracked (`src/main.rs` builds every pack against its installed CLI and records what each node sends; `analyse.py` compares that with the command's parameter list; the raw output is in `out/`).

## Short answer

The suspicion is right, and it is two problems, not one.

1. **Most commands have no node at all.** The 13 packs run 274 of 7,426 commands (about 4 %). 4,354 of the commands no node runs take parameters. Every pack has an escape hatch (`<x>.run`) that runs any allowed command with raw JSON, so those commands can be reached, but not as widgets.
2. **The nodes that do exist hide parameters.** 161 of 295 node commands have at least one parameter no widget, file socket or fixed value sends: 737 of their 1,156 parameters. A good share of that is noise (below), but a real core remains, and some of it produces wrong results, not just missing options.

## By craft

| Craft | Commands | Run by a node | Not run, with params | Node commands | With hidden params | Hidden params | Params on node commands |
|---|---|---|---|---|---|---|---|
| CADCraft | 449 | 23 | 231 | 26 | 9 | 27 | 52 |
| DeckCraft | 331 | 21 | 186 | 25 | 13 | 72 | 103 |
| DesignCraft | 545 | 22 | 274 | 23 | 16 | 55 | 81 |
| EffectCraft | 623 | 13 | 305 | 13 | 5 | 10 | 14 |
| FigCraft | 629 | 28 | 128 | 30 | 1 | 1 | 2 |
| FilmCraft | 716 | 17 | 505 | 20 | 16 | 149 | 208 |
| GridCraft | 364 | 18 | 177 | 19 | 11 | 16 | 44 |
| PdfCraft | 209 | 19 | 50 | 19 | 14 | 48 | 101 |
| PhotoCraft | 953 | 27 | 742 | 27 | 12 | 45 | 89 |
| SolidCraft | 570 | 17 | 498 | 20 | 18 | 60 | 96 |
| SoundCraft | 483 | 15 | 247 | 18 | 11 | 13 | 33 |
| VectorCraft | 906 | 33 | 629 | 34 | 26 | 220 | 290 |
| WordCraft | 648 | 21 | 382 | 21 | 9 | 21 | 43 |

VectorCraft also generates 130 effect nodes from its effect catalogue. No other pack generates nodes.

## How much of the "hidden" count is real

The parameter lists are read from each craft's own descriptions: JSON Schema for PdfCraft, prose for the rest. The prose reader is lenient, so the counts are an estimate. Four kinds of noise inflate them:

- **One export command shared by several formats.** DeckCraft's `file.export`, VectorCraft's `document.export` and FilmCraft's `file.exportMedia` take every format's options, so the PNG node "hides" `secondsPerSlide`, and the WAV node hides `crf`. This is most of DeckCraft's, VectorCraft's and FilmCraft's count.
- **Selection plumbing.** `ids`, `objects` and `items` on VectorCraft's and FilmCraft's edits are what the node's `select` port sets.
- **Reply fields misread as parameters.** For example, SolidCraft's `parameters.change` lists `changed`, `recomputed`, and WordCraft's `file.inspect` lists `found`, `residual`.
- **Deliberately left out.** For example, `warnings` and `documentFonts` on open.

## Hidden parameters that matter

These change what a node can make, or make it silently wrong:

- **FilmCraft exports.**
  - MOV, GIF and WAV have no range (`range`, `startSeconds`, `endSeconds`), so they always export the whole sequence. Values for them saved in a workflow are dropped.
  - MOV has no `audio` switch.
  - No export has `width`, `height` or `fps`, or a ProRes profile.
  - `fc.place` has no `time`, `sourceIn` or `duration`, so clips can only go at the default place.
  - `fc.text` and `fc.shape` have no `track` or `time`.
- **PhotoCraft adjustments.**
  - Black & White and Hue/Saturation have no per-colour sliders: `reds` … `magentas`, `tint`.
  - Levels has no per-channel `red`, `green` or `blue`.
  - Crop has no `angle`, target size or resolution.
- **SolidCraft.**
  - Fillet has none of its variable-radius options (`type`, `radii`).
  - Chamfer has no `distance2` or `angle`.
  - Sphere has no `diameter`; mirror and pattern have no `features`, so they can't copy features, only bodies.
  - The exports have no `quality`, so STL and 3MF tessellation is always the default.
- **PdfCraft.**
  - Add Text has no `font`, `bold`, `italic`, `align` or `rect`.
  - Watermark has no `color`, `font_size`, `pages` or offsets, and no file watermark.
  - Background has no `opacity` or `pages`.
  - Several page operations can't name `pages`.
- **DesignCraft.**
  - PDF export has no `bleed`, `marks`, `spreads`, `tagged` or image compression.
  - Find/Change has no `paraStyle` or `charStyle`.
- **DeckCraft.**
  - Background has no `picture` or `transparency`.
  - PNG and PDF export can't choose slides (`slides`, `slide`, `all`).
- **VectorCraft.**
  - The stroke has no arrowheads or width profile.
  - Fill has no swatch, tint or gradient.
  - Scale and rotate have no options for patterns and type areas.
- **WordCraft.**
  - Margins: the node only offers presets, with no `top`, `bottom`, `left`, `right`, `gutter` or `mirror`.
  - Line spacing has no `exactly` or `atLeast`.
  - Paper size has no custom `width` or `height`.
- **Smaller gaps:**
  - GridCraft: the PDF export's `range` and `fitToPage`, and the pivot table's `filters` and `destination`.
  - SoundCraft: the plug-in `slot` on inserts.
  - EffectCraft: transparent PNG frames.

## Areas with no nodes at all

The command groups with the most parameter-taking commands that no node runs:

- **PhotoCraft:** `layer` (181), `filter` (106), `type` (65).
  - Five filters have nodes (Gaussian Blur, Add Noise, Unsharp Mask, Clouds, Oil Paint); over 100 have none.
- **SolidCraft:** `sketch` (124), `render` (53), `component` (30).
  - No sketch nodes, so a part can only be built from primitives.
- **VectorCraft:** `object` (90), `addon` (54), `path` (28).
- **FilmCraft:** `graphics` (57), `clip` (44), `sequence` (37), `multicam` (33), `markers` (29).
- **DesignCraft:** `table` (38), `object` (33), `type` (26).
- **WordCraft:** `insert` (33), `table` (29), `para` (22).
- **EffectCraft:** `layer` (54), `keys` (16), `renderQueue` (15).
  - Solid, shape and text layers and their transform have nodes; every other layer command is only through `ec.run`.
- **DeckCraft:** `format` (33), `shape` (29), `insert` (15), `table` (13).
- **GridCraft:** `home` (25), `data` (22), `pivot` (17).
- **CADCraft:** spread thinly; no area has more than 5.

## What would fix it

1. **Export nodes take the options for their own format:**
   - Use `options_from` (the catalogue route VectorCraft already uses), or the pack lists per-format option names.
   - At minimum: FilmCraft range, size and fps; DeckCraft slides; SolidCraft quality.
   - The FilmCraft range is the one that gives wrong results today.
2. **Generate a node's remaining parameters as `advanced` widgets** from the craft's own parameter list, the way VectorCraft's effect nodes are generated. The pack would then only list what it renames, hides or fixes.
   - The catalogue check already knows each command's accepted keys (`accepted()` in `crates/craft/src/lib.rs`).
   - That check is what this probe uses.
3. **Generate whole families of nodes from catalogues,** as VectorCraft's effects are. The obvious next ones:
   - PhotoCraft's filters and adjustments;
   - SolidCraft's sketch commands;
   - EffectCraft's layer commands.
4. **Make the probe a test,** so a craft that adds a parameter shows up as a hidden one. It could report the count, or fail above a threshold.
