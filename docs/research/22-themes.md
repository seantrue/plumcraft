---
title: "Themes, fruit headers and type"
subtitle: "Research 22: colour schemes and typography for the PlumCraft editor"
date: 2026-11-05
---

# Themes, fruit headers and type

Status: implemented 2026-11-05 (see `docs/STATUS.md`), with these deviations from the proposal: the window is File › Settings… (`app.settings`, ⌘,), with Appearance, Engines and Cache tabs, not View › Appearance; its commands are `view.theme`, `view.textSize`, `view.zoom`, `theme.setToken`, `theme.setFruit`, `theme.pinFamily`, `theme.reset`, `theme.import` and `theme.export`; an imported theme file becomes changes to its base theme rather than a separately named user theme; there are no palette sliders and no font choice yet; net tags stay 11 pt monospace. Medium and Dark were mapped to Pewter and Dusk (open question 2), and the other open questions were answered as proposed.

Original status: proposal, nothing implemented. Asked for by the maintainer: "The current color scheme is very dark (echos photocraft) and fonts are a little small and low contrast. Look at the color schemes from other dataflow applications … and propose an alternative. Desaturated fruit colors would be nice for node headings … out-of-the-box themes that are attractive is more important" than a detailed settings pane.

In short:

- Ship three themes: **Paper** (warm light, the proposed default), **Pewter** (mid grey, a lighter and crisper Medium) and **Dusk** (an improved dark). Every `Tokens` field has a value below.
- Node headers take one of twelve named, desaturated **fruit** colours (plus **olive**, a quiet one, and **plum**, kept for groups). The fruits are set in OKLCH at matched lightness and chroma per theme, and alternate a lighter and a deeper tier so that neighbouring hues still differ.
- A family's id hashes into the twelve, with a probe inside each catalogue so that two families in one engine never share a fruit. An engine's own colour still wins, but takes the theme's lightness and chroma.
- Text: **Inter** becomes the primary face (it's already bundled, but today only as a fallback behind Ubuntu Light), and body text goes from 12.5 to 14 pt.
- All 117 text and UI contrast pairs pass WCAG 2: 4.5:1 for text, 3:1 for headers' secondary text, borders and run-state rings (see the Verification section).
- A settings pane (Appearance) comes second: theme, text size, per-token overrides, fruit editing and theme files.

The probe that computes everything here is `claude-debug/theme_contrast_probe.py`. It writes the swatches and `claude-debug/themes/palettes.json`.

## 1. How other dataflow editors look

Colour values come from source where it's public: Node-RED's `colors.scss` and node `.html` files, Blender's `userdef_default_theme.c`, and ComfyUI's `src/assets/palettes/*.json` and LiteGraph. The other values come from documentation and screenshots. Values marked "about" were read from screenshots, not from source.

| Editor | Ground | Canvas / node / header | How categories are coloured | Type | Theming offered |
|---|---|---|---|---|---|
| Node-RED | light | workspace white on #f0f0f0 chrome; whole node filled with its colour, label #333, border #999, wires #999 | pastel fill per node kind: inject #a6bbcf, function #fdd0a2, debug #87a980, switch/change #E2D96E, JSON/CSV #DEBD5C, mqtt #d8bfd8, delay #E6E0F8, complete #c0edc0, template rgb(243,181,103), file BurlyWood, exec darksalmon | Helvetica Neue / Arial 14px, port labels #646464, small labels 9pt | `editorTheme` in settings, theme plugins (the community theme collection has dark ones); colours are SCSS variables |
| Ab Initio GDE | light | white Windows-style canvas, components as rectangles with icons, flows as lines | little category colour; colour is mostly status: run LEDs (green, red with the error on hover), yellow highlights for settings still to do | system UI font | essentially none |
| KNIME (5.x) | light | light-grey canvas, small square node icons on a coloured tile, name under the node, traffic-light state under that | colour per node type: orange sources/readers, yellow manipulators, red sinks/writers, grey components, purple scope, light blue loops (forum moderator: "general guidance, not hard rules") | system sans, about 12px labels | node annotations with free colour; no editor theme |
| Apache NiFi | light (2.x adds dark) | white processor boxes with a grey title bar, status icons and counters, grid canvas | none by type; the user can set a component's fill | Roboto-style sans, about 12px | per-component colour; dark mode in 2.x |
| LabVIEW block diagram | light | white diagram, grey structures, small icon nodes | **wires** coloured and patterned by type: orange float, blue integer, pink string, green Boolean, thicker for arrays | Application font about 13px | wire and type colours fixed; the diagram background is configurable |
| Max/MSP | dark (Max 8 and 9) | dark-grey patcher, rounded object boxes, light text | cord style by domain: grey control, striped yellow for MSP audio, striped green for Jitter | Lato in Max 8 and later, about 12pt | full colour themes and a per-object format palette |
| Pure Data | light | white canvas, black boxes and lines | none; signal cords thicker | DejaVu Sans Mono 10 to 12, set per patch | font size per patch, little else |
| Blender node editor | dark | backdrop #1a1a1a with a #303030 grid, bodies about #303030, header bands dark and muted | per class, all low-chroma and dark: shader #2b652b, texture #79461d, vector #3c3c83, geometry #1d725e, filter #412b51, output #3e232a, script #203c3c, attribute #1d2546 | DejaVu Sans / Inter (4.x), resolution scale | the most complete: Preferences › Themes edits every colour, and presets are XML files |
| Unreal Blueprints | dark | dark grid, nodes with a gradient header and a translucent body | header by node kind (red event, blue call, green pure, grey flow control); pins and wires by type | Roboto about 10pt | pin and wire colours in Editor Preferences › Graph Editor |
| Houdini | mid/dark | mid-grey network editor, node shapes by kind, flags on the node | the user picks colours from a fixed swatch palette; some defaults by node kind | DejaVu-style sans, scalable | colour schemes are `.hcs` files (Houdini Dark, Light, Pro) |
| Nuke | dark | dark-grey node graph, rounded tiles | tile colour by class (Read/Write, Merge, Colour, Filter, Transform, 3D each have their own hue), overridable per node | system sans, about 11px | preferences for per-class colours; the user can colour any node |
| TouchDesigner | dark | dark network, tiles with live viewers | by operator family: TOP purple, CHOP green, SOP blue, DAT pink, MAT yellow, COMP grey | Verdana-like, about 11px | UI colour settings, rarely changed |
| ComfyUI (LiteGraph) | dark (light palette ships) | dark: background #202020, node body #353535, title bar #333, title text #999, links #9A9; light: #DDD / #F5F5F5 / #222 | nine user-picked title/body pairs: red #322/#533, brown #332922/#593930, green #232/#353, blue #223/#335, pale blue #2a363b/#3f5159, cyan #233/#355, purple #323/#535, yellow #432/#653, black #222/#000 | Arial 14px titles, 12px widgets | colour palettes as JSON files you can import and export (dark, light, arc, github, …) |
| Orange | light | white canvas, round widget icons, name under the icon | soft colour per category (Data, Transform, Visualize, Model, Evaluate, Unsupervised) | system sans | none |
| Enso | both | nodes as pills, wires coloured | by the library or type group a method comes from | its own sans | light and dark |
| n8n | light (dark available) | dotted light canvas, white nodes with brand icons | colour only from icons; trigger nodes have a rounded left edge | Open Sans-style, 14px | light, dark or system |
| Alteryx Designer | light | white canvas, round tool icons | the icon colour follows the palette category (for example green In/Out, blue Preparation, purple Join) | Segoe UI | little |

What this shows:

- **Every data-integration and flow tool is light** (Node-RED, GDE, KNIME, NiFi, LabVIEW, Orange, n8n, Alteryx). Only media tools are dark (Blender, Nuke, Houdini, TouchDesigner, Unreal, ComfyUI), and they're dark because the work is pictures, where a dark surround helps judge tone. PlumCraft does both: node values are often pictures, but the graph itself is the document.
- **Pastel categories on light, dark and muted ones on dark.** Node-RED's fills are around OKLCH L 0.80 to 0.90 with low chroma. Blender's and LiteGraph's headers are around L 0.30 to 0.45 with low chroma. Nobody uses saturated header colours. The fruit palette follows both, with one hue table and a lightness and chroma per theme.
- **Category means family.** KNIME, TouchDesigner, Blender, Nuke and Node-RED all colour by what kind of node it is. That's what PlumCraft's `family_tint` already does. Type colours belong on pins and wires (LabVIEW, Unreal), which PlumCraft also does.
- **Theme files are normal.** Blender (XML), Houdini (`.hcs`) and ComfyUI (JSON) all import and export theme files, and Node-RED's themes are packages. Blender's full editor is admired but little used. The defaults matter more, as the maintainer says.

## 2. What's wrong with the current themes

Measured with the probe on `theme.rs` at `2005891`, and on `snapshot` renders of the demo graph (`claude-debug/themes/current-medium.png`, `current-dark.png`).

| Theme | Pair | Foreground | Background | Ratio |
|---|---|---|---|---|
| Medium | text on panel | #eeeeee | #535353 | 6.63 |
| Medium | text_faint on panel | #aaaaaa | #535353 | 3.31 |
| Medium | text on a node header (rendered) | #eeeeee | #947e37 | 3.42 |
| Medium | text_dim on a node header (timing) | #d4d4d4 | #947e37 | 2.67 |
| Medium | text_faint on a node body | #aaaaaa | #5f5b4f | 2.92 |
| Medium | panel against canvas (rendered) | #535353 | #454545 | 1.25 |
| Medium | node body against canvas | #5f5b4f | #454545 | 1.41 |
| Dark | text on a node header (rendered) | #dcdee2 | #877333 | 3.44 |
| Dark | text_dim on a node header | #a6aab0 | #877333 | 1.99 |
| Dark | panel against canvas | #25282d | #1f2125 | 1.09 |
| Dark | node body against canvas | #35342c | #1f2125 | 1.29 |

The diagnosis:

1. **Everything is dark or mid-dark.** Medium's surfaces sit at OKLCH L 0.39 to 0.47 and Dark's at 0.25 to 0.32. Nothing is above L 0.6 except text, so the window reads as one dark mass.
2. **Headers are the weakest text in the window.** Titles reach only 3.4:1 (the tests ask for 3:1), and the timing and status text beside them is 2.0 to 2.7:1, below any WCAG level. Mixing 55 to 60% of a mid-value hue into grey lands every header around L 0.56 to 0.60. That's the worst lightness for light text.
3. **Surfaces don't separate.** Panel against canvas is 1.09 to 1.25:1 and node body against canvas is 1.3 to 1.4:1. Nodes float on shadow, not on contrast.
4. **Type is small and thin.** The body size is egui's default of 12.5 pt and the face is Ubuntu Light (weight 300). Inter is bundled but only fills glyphs Ubuntu lacks. Small is 10.5 pt and canvas captions are 10 to 11 pt. A light weight at 12.5 pt on a mid-grey ground is what reads as "small and low contrast", even where the ratio is 6.6:1.
5. **Family colour carries little.** Hues come from a free 0 to 360 hash at HSV S 0.5 and V 0.62, then mixed into grey. Hues closer than about 30 degrees look the same, and the demo graph is a single family, so every header is the same mustard.

## 3. The proposed themes

Three themes, each a full `Tokens`. **Paper** is proposed as the default. Pewter replaces Medium (the menu could keep "Medium" as an alias for old prefs), and Dusk replaces Dark.

Three tokens are new:

- `header_text`: text on a header. It's dark on Paper and white on Pewter.
- `wire`: the default wire colour.
- `grid`: the canvas dots.

`family_saturation` and `family_value` become the palette's OKLCH lightness and chroma (`fruit_l`, `fruit_dl`, `fruit_c`). `apply()` must start from `Visuals::light()` for Paper (set `dark_mode` by theme) instead of always forcing dark.

The swatches are mock graphs (header, body, ports, wires, run-state rings, an inspector strip and the fruit set) drawn from these tokens by the probe.

![Paper](../../claude-debug/themes/swatch-paper.png)

![Pewter](../../claude-debug/themes/swatch-pewter.png)

![Dusk](../../claude-debug/themes/swatch-dusk.png)

### Paper: warm light, like GDE or Node-RED

A warm off-white with a light-grey paper canvas, ink text and pastel fruit headers with dark titles. The warmth (hue about 80, chroma 0.01) keeps it from looking clinical, and it's the look of a paper drawing or of the light enterprise tools. Pictures in thumbnails still read well, since Node-RED and KNIME show charts on light.

### Pewter: mid grey

This is Medium done right. The canvas is lifted from #282828 (rendered #454545) to #4b4d52, the panel sits darker than the canvas so the docks recede, and fields have a visible 3.5:1 border. Headers are deeper (L about 0.50) so that white titles reach 5.1:1 or better. Pewter is for people who keep image work on a neutral grey.

### Dusk: improved dark

The research software's dark theme with the problems fixed. Text is warmer and brighter, borders reach 3.4:1 or better, and headers are L about 0.43 so titles reach 6.1:1 or better. The canvas and the panel step apart.

### Every token

| Token | Paper | Pewter | Dusk |
|---|---|---|---|
| name / label | paper / Paper | pewter / Pewter | dusk / Dusk |
| canvas | #e9e5dc | #4b4d52 | #202226 |
| panel | #f5f3ee | #43454a | #2a2c31 |
| text | #22201c | #f5f4f1 | #eceae6 |
| text_dim | #57524a | #d6d4cf | #bab7b1 |
| text_faint | #7f796f | #bdbbb6 | #8d8b86 |
| separator | #d3cdc2 | #36383c | #18191c |
| field | #ffffff | #3a3c40 | #232529 |
| field_border | #8a8377 | #9a9ca1 | #7a7e87 |
| section | #ebe7df | #3e4044 | #31343a |
| accent | #2c66ad | #94c2f7 | #6c9fe0 |
| error | #b3342a | #fbc2ba | #ec7d70 |
| warning | #8f6200 | #ebb85e | #ddae55 |
| banner | #f4d8d1 | #7a3a33 | #6b2e28 |
| widget.noninteractive | #f5f3ee | #43454a | #2a2c31 |
| widget.inactive | #e8e4dc | #55575c | #363940 |
| widget.hovered | #ddd8ce | #66686d | #41454c |
| widget.active | #d0cabe | #6a6d72 | #4c5058 |
| widget.open | #ddd8ce | #66686d | #41454c |
| radius | 5 | 5 | 5 |
| pin_default | #7f796f | #b4b2ad | #a3a19c |
| pin_hidden | transparent | transparent | transparent |
| state_queued | #7d838d | #a1a6ae | #7a808a |
| state_running | #2c66ad | #94c2f7 | #6c9fe0 |
| state_done | #3a8a4c | #82d290 | #68ba77 |
| state_cached | #6f9c78 | #5f9a6b | #467a51 |
| state_error | #c0392b | #fbc2ba | #ec7d70 |
| state_skipped | #a29d94 | #85888d | #5c6067 |
| state_cancelled | #a29d94 | #85888d | #5c6067 |
| stale | #b07d12 | #ebb85e | #ddae55 |
| highlight | #d4a300 | #fff3b0 | #ecd65a |
| placeholder | #e2ddd3 | #45474c | #2f3237 |
| faded | rgba(245,243,238,0.55) | rgba(110,110,110,0.43) | rgba(110,110,110,0.43) |
| chart | #2c66ad | #94c2f7 | #6c9fe0 |
| waveform | #1d7468 | #62d6c3 | #45c2ae |
| waveform_bg | #fbfaf7 | #2f3134 | #17181b |
| playhead | #2a2722 | #fff3b0 | #ecd65a |
| spectrogram | magma: #000004 #3b0f70 #8c2981 #de4968 #fe9f6d #fcfdbf | same | same |
| family_default.header | #ddd8cf | #64676c | #3d4047 |
| family_default.body | #fbfaf6 | #57595e | #2e3136 |
| group (plum) header | #edb8e7 | #7d5279 | #664162 |
| family_body_mix | 0.10 | 0.10 | 0.10 |
| header_text (new) | #22201c | #ffffff | #f4f2ee |
| wire (new) | #7d786f | #b4b2ad | #8d8b86 |
| grid (new) | #ddd8ce | #56585d | #282a2f |
| fruit_l / fruit_dl / fruit_c (replace saturation, value) | 0.845 / 0.035 / 0.074 | 0.500 / 0.030 / 0.070 | 0.430 / 0.032 / 0.062 |

Group bodies are `mix(family_default.body, group.header, 0.10)`, as for families.

The spectrogram keeps magma in every theme. The tests need a dark-to-light ramp, and on Paper it sits in its own `waveform_bg` box. Paper's playhead is ink rather than yellow, because yellow can't reach 4.5:1 on a white ground.

## 4. The fruit palette

Twelve family colours, with hues set about 30 degrees apart in OKLCH. Adjacent fruits alternate between a lighter and a deeper tier (±`fruit_dl`), so even the two closest hues differ in lightness. Within a theme every fruit has the same chroma. Two fruits are outside the family set:

- **olive**: a low-chroma, near-neutral one for a family the user wants quiet.
- **plum**: reserved for groups, as PlumCraft's own colour.

| Fruit | OKLCH hue | Tier | Paper | Pewter | Dusk |
|---|---|---|---|---|---|
| cherry | 22 | light | #fec8c5 | #905b59 | #784a49 |
| apricot | 58 | deep | #e6b692 | #785131 | #603e24 |
| quince | 84 | light | #efd4a0 | #7f683a | #69562e |
| lemon | 104 | deep | #c8c48c | #615d2b | #4d491e |
| pear | 124 | light | #cde0ab | #637345 | #515f37 |
| greengage | 148 | deep | #a1cfa6 | #3e6644 | #2f5034 |
| melon | 176 | light | #a3e8d6 | #3a796a | #2d6457 |
| juniper | 208 | deep | #86cfda | #1e666f | #115059 |
| blueberry | 248 | light | #b9dcfe | #4a6f93 | #3c5c7a |
| damson | 276 | deep | #b3bef1 | #505782 | #3e4468 |
| grape | 302 | light | #e0cdff | #73628e | #5f5076 |
| fig | 356 | deep | #e9aec2 | #7a4a5c | #623948 |
| olive (quiet) | 112 | neutral | #b0b29a | #696b56 | #555745 |
| plum (groups) | 330 | neutral | #edb8e7 | #7d5279 | #664162 |

How distinguishable they are (OKLab distance ×100; about 2 is just noticeable, 5 is clearly different at a glance):

| Theme | Closest family pair | Next closest | Header lightness range | Plum against grape / fig |
|---|---|---|---|---|
| Paper | 5.1 (quince / pear) | 5.5 (lemon / greengage) | 0.810 to 0.880 | 5.4 / 5.1 |
| Pewter | 4.9 (quince / pear) | 5.2 (lemon / greengage) | 0.469 to 0.530 | 4.8 / 4.6 |
| Dusk | 4.3 (quince / pear) | 4.6 (lemon / greengage) | 0.397 to 0.463 | 4.6 / 4.4 |

An earlier version had 15 hues at a single lightness and chroma, and its neighbours were only 1.7 apart, too close to tell at a glance. At this low chroma, twelve hues with alternating tiers is about the most the eye can name. Sixteen would need either more chroma (no longer "desaturated") or a second lightness level that would read as another meaning.

### Which family gets which fruit

1. **An engine colour wins**, as today, but only for its **hue**. The engine's `#rrggbb` is converted to OKLCH and re-rendered at the theme's `fruit_l` and `fruit_c`. An engine that says gold still gets gold, in each theme's register. Today a saturated engine colour is mixed 55 to 60% into grey and reaches only 3.4:1.
2. **Otherwise the family id hashes into the twelve**: `FAMILY_FRUITS[fnv1a(id) % 12]`, with the FNV-1a already pinned in `theme.rs`.
3. **Probe within a catalogue.** Eight families in twelve slots will usually collide (in the probe, `filter` and `table` both hash to fig). So the families of one catalogue are taken in id order, each takes its hash slot, and a clash steps by 5 (coprime with 12) to the next free fruit. This is stable for a given catalogue and independent of catalogue order. Adding a family can move at most the families that clashed with it. With more than twelve families, fruits are shared.
4. **The user can pin a family** to a fruit or a hex in prefs (section 6). A pin beats the hash, and an engine colour beats a pin only if the user hasn't pinned.

| Family | Hue now (degrees) | Fruit by hash | Fruit by hash and probe |
|---|---|---|---|
| source | 56 | blueberry | blueberry |
| output | 148 | pear | pear |
| text | 94 | grape | grape |
| audio | 321 | damson | damson |
| image | 218 | quince | quince |
| filter | 167 | fig | fig |
| math | 55 | juniper | juniper |
| table | 263 | fig | cherry |

The fruit names are user-facing: in the settings pane, in a theme file, and in a possible later catalogue hint where an engine says `"color": "fruit:pear"` instead of a hex. That hint would be an interface change, a capability, and isn't proposed now.

Pins in Pewter and Dusk should be drawn from a lighter step of the fruit (`fruit_l + 0.25`) rather than the header colour itself. The swatches show that header-coloured pins are dim on dark bodies.

## 5. Typography

| Text style | Now | Proposed | Notes |
|---|---|---|---|
| Body (`TextStyle::Body`, `Button`) | 12.5 pt Ubuntu Light | 14 pt Inter Regular | panels, inspector, menus |
| Small | 10.5 pt | 12 pt | type tags, ids, timings |
| Heading | 15 pt | 17 pt Inter SemiBold | panel titles |
| Monospace | 12 pt Hack | 13 pt (Hack, or JetBrains Mono) | nets, code, values |
| Node title | Body | 14 pt Inter SemiBold | header weight, not size, sets it apart |
| Port labels | Body | 13 pt Inter Regular | one step under the title |
| Node captions and preview text | 10 to 11 pt | 12.5 pt, `text_dim` | `canvas.rs`, `audio.rs`, `compare_view.rs` literals |
| Net tags | 11 pt mono | 12 pt mono | `net_view.rs` |

All of these scale with the canvas zoom as today, and with one global text scale (section 6). The hard-coded `FontId::proportional(10.0 / 11.0 / 12.0)` literals should become named sizes read from the theme, as colours already are.

Choosing the face:

- **Inter** (SIL OFL 1.1, already in `assets/fonts` with its licence). It was designed for screen UI at 11 to 16 px, has a large x-height and open apertures, and covers Latin, Greek and Cyrillic. It needs Medium or SemiBold added (about 300 KB each) for titles. Proposed as the primary proportional face, with Ubuntu Light dropped or kept as a fallback.
- **IBM Plex Sans** (OFL). It has more character, slightly wider and more "engineering", and pairs with IBM Plex Mono. A good second choice if Inter feels generic.
- **Atkinson Hyperlegible Next** (OFL, Braille Institute, 2025). Its distinct `l I 1` and `0 O` help with ids and parameter values. Offer it in the settings pane as a "Legible" choice rather than as the default, since its rhythm is less even in dense panels.
- **Source Sans 3** and **Noto Sans** (OFL) are fine but bring nothing Inter lacks. **Ubuntu** (Ubuntu Font Licence) at Regular weight would be acceptable, but the Light weight egui ships is the problem.
- **Monospace**: keep **Hack** (MIT plus Bitstream Vera), or move to **JetBrains Mono** (OFL), which has a taller x-height and matches Inter better.

egui can't turn on OpenType features, so Inter's tabular figures (`tnum`) aren't available. Columns of timings stay proportional unless they're drawn in monospace.

## 6. Settings pane (secondary to the defaults)

View › Appearance… opens a window, and every control in it is a command (`view.appearance`, `view.theme.set {name}`, `theme.import`, `theme.export`, `theme.reset_token {token}`), as the golden rules ask.

1. **Theme**: a list of built-in and user themes, each with a small preview (the swatch). Duplicate a theme to edit it. Built-ins are read-only.
2. **Text**: the face (Inter, IBM Plex Sans, Atkinson Hyperlegible Next, plus the monospace face), a base size of 11 to 18 pt with every other size derived by the ratios in section 5, and a UI zoom of 0.8 to 1.6 (egui `zoom_factor`).
3. **Colours**: the tokens grouped as Surfaces, Text, Controls, Run states, Media and Canvas. Each has a colour picker and a reset button. Every text token shows a live contrast badge against its backgrounds (for example "4.9:1 on panel"), amber below 4.5 and red below 3. These are the same checks as the unit tests, so a user theme can't quietly become unreadable.
4. **Node families**: the fruit swatches, editable per theme, the three palette numbers (`fruit_l`, `fruit_dl`, `fruit_c`) as sliders, and a per-family table that lists the current catalogue's families with their fruit and a "pin to" picker (fruit name or hex).

Where it's kept. The current `Prefs` stays the store, and its `theme` field keeps its meaning. New keys go in an `appearance` object, and unknown keys are kept, as `extra` already does:

```json
{
  "theme": "paper",
  "appearance": {
    "font": "inter", "mono": "hack", "text_size": 14.0, "zoom": 1.0,
    "overrides": { "paper": { "accent": "#2c66ad" } },
    "families": { "vi.filter": "juniper", "x.audio": "#a3e8d6" }
  }
}
```

Theme files. A user theme is `<config dir>/themes/<name>.plumtheme.json`, laid out as follows:

```json
{
  "format": "plumcraft-theme", "version": 1, "name": "my-paper", "label": "My Paper",
  "base": "paper", "tokens": { "canvas": "#e6e1d6" }, "fruits": { "pear": "#cfe0a8" },
  "fruit_l": 0.845, "fruit_dl": 0.035, "fruit_c": 0.074
}
```

Only the differences from `base` are stored, so a theme file stays small and keeps up with fixes to the built-ins. Import and export go through the file dialog (with the `prefs::keys` folder memory). A theme file is hostile input like any other:

- a bad hex or an unknown token is skipped, and an unknown `format` or `version` is refused with a message, never a panic;
- the size is capped as `prefs.json` is;
- the parser joins the hostile-input sweep.

## 7. Verification

`claude-debug/theme_contrast_probe.py` builds the themes and fruits from OKLCH (reducing chroma until each colour is in sRGB gamut) and checks WCAG 2 contrast. Text pairs need 4.5:1, header secondary text and faint text 3:1, and borders, rings, wires and pins 3:1 (WCAG 1.4.11). The probe returns non-zero on any failure. Run it like this:

```sh
python3 claude-debug/theme_contrast_probe.py > claude-debug/themes/probe-out.md
```

The final run: **117 pairs, 0 below target.** The table below is the probe's output. "Worst fruit" rows take the minimum over all 14 fruits (for bodies, a 10% fruit mix into `family_default.body`).

| theme | pair | fg | bg | ratio | needs | result |
|---|---|---|---|---|---|---|
| paper | text on panel | #22201c | #f5f3ee | 14.66 | 4.5 | pass |
| paper | text_dim on panel | #57524a | #f5f3ee | 6.99 | 4.5 | pass |
| paper | text_faint on panel | #7f796f | #f5f3ee | 3.89 | 3.0 | pass |
| paper | text on section | #22201c | #ebe7df | 13.19 | 4.5 | pass |
| paper | text_dim on section | #57524a | #ebe7df | 6.28 | 4.5 | pass |
| paper | text on field | #22201c | #ffffff | 16.26 | 4.5 | pass |
| paper | text_dim on field | #57524a | #ffffff | 7.75 | 4.5 | pass |
| paper | text on canvas | #22201c | #e9e5dc | 12.94 | 4.5 | pass |
| paper | text_dim on canvas | #57524a | #e9e5dc | 6.16 | 4.5 | pass |
| paper | text on w_inactive | #22201c | #e8e4dc | 12.82 | 4.5 | pass |
| paper | text on w_hovered | #22201c | #ddd8ce | 11.45 | 4.5 | pass |
| paper | text on w_active | #22201c | #d0cabe | 9.97 | 4.5 | pass |
| paper | text on banner | #22201c | #f4d8d1 | 12.07 | 4.5 | pass |
| paper | accent on panel | #2c66ad | #f5f3ee | 5.24 | 4.5 | pass |
| paper | error on panel | #b3342a | #f5f3ee | 5.50 | 4.5 | pass |
| paper | warning on panel | #8f6200 | #f5f3ee | 4.84 | 3.0 | pass |
| paper | error on default_body | #b3342a | #fbfaf6 | 5.84 | 4.5 | pass |
| paper | text on default_body | #22201c | #fbfaf6 | 15.57 | 4.5 | pass |
| paper | text_dim on default_body | #57524a | #fbfaf6 | 7.42 | 4.5 | pass |
| paper | text_faint on default_body | #7f796f | #fbfaf6 | 4.13 | 3.0 | pass |
| paper | title_text on default_header | #22201c | #ddd8cf | 11.46 | 4.5 | pass |
| paper | field_border vs field (UI) | #8a8377 | #ffffff | 3.75 | 3.0 | pass |
| paper | field_border vs panel (UI) | #8a8377 | #f5f3ee | 3.39 | 3.0 | pass |
| paper | accent vs canvas (UI) | #2c66ad | #e9e5dc | 4.63 | 3.0 | pass |
| paper | state_done vs canvas (UI) | #3a8a4c | #e9e5dc | 3.40 | 3.0 | pass |
| paper | state_error vs canvas (UI) | #c0392b | #e9e5dc | 4.33 | 3.0 | pass |
| paper | state_running vs canvas (UI) | #2c66ad | #e9e5dc | 4.63 | 3.0 | pass |
| paper | state_queued vs canvas (UI) | #7d838d | #e9e5dc | 3.04 | 3.0 | pass |
| paper | highlight vs canvas (UI) | #d4a300 | #e9e5dc | 1.85 | 1.8 | pass |
| paper | wire vs canvas (UI) | #7d786f | #e9e5dc | 3.49 | 3.0 | pass |
| paper | pin_default vs default_body (UI) | #7f796f | #fbfaf6 | 4.13 | 3.0 | pass |
| paper | waveform vs waveform_bg (UI) | #1d7468 | #fbfaf7 | 5.36 | 4.5 | pass |
| paper | playhead vs waveform_bg (UI) | #2a2722 | #fbfaf7 | 14.25 | 4.5 | pass |
| paper | playhead vs waveform (UI) | #2a2722 | #1d7468 | 2.66 | 1.3 | pass |
| paper | chart vs panel (UI) | #2c66ad | #f5f3ee | 5.24 | 3.0 | pass |
| paper | title_text on worst fruit header | #22201c | (14 fruits) | 7.50 | 4.5 | pass |
| paper | text_dim on worst fruit header | #57524a | (14 fruits) | 3.57 | 3.0 | pass |
| paper | text on worst fruit body | #22201c | (14 fruits) | 14.63 | 4.5 | pass |
| paper | text_faint on worst fruit body | #7f796f | (14 fruits) | 3.88 | 3.0 | pass |
| pewter | text on panel | #f5f4f1 | #43454a | 8.72 | 4.5 | pass |
| pewter | text_dim on panel | #d6d4cf | #43454a | 6.48 | 4.5 | pass |
| pewter | text_faint on panel | #bdbbb6 | #43454a | 5.00 | 3.0 | pass |
| pewter | text on section | #f5f4f1 | #3e4044 | 9.44 | 4.5 | pass |
| pewter | text_dim on section | #d6d4cf | #3e4044 | 7.01 | 4.5 | pass |
| pewter | text on field | #f5f4f1 | #3a3c40 | 10.05 | 4.5 | pass |
| pewter | text_dim on field | #d6d4cf | #3a3c40 | 7.46 | 4.5 | pass |
| pewter | text on canvas | #f5f4f1 | #4b4d52 | 7.69 | 4.5 | pass |
| pewter | text_dim on canvas | #d6d4cf | #4b4d52 | 5.71 | 4.5 | pass |
| pewter | text on w_inactive | #f5f4f1 | #55575c | 6.57 | 4.5 | pass |
| pewter | text on w_hovered | #f5f4f1 | #66686d | 5.07 | 4.5 | pass |
| pewter | text on w_active | #f5f4f1 | #6a6d72 | 4.72 | 4.5 | pass |
| pewter | text on banner | #f5f4f1 | #7a3a33 | 7.70 | 4.5 | pass |
| pewter | accent on panel | #94c2f7 | #43454a | 5.17 | 4.5 | pass |
| pewter | error on panel | #fbc2ba | #43454a | 6.18 | 4.5 | pass |
| pewter | warning on panel | #ebb85e | #43454a | 5.28 | 3.0 | pass |
| pewter | error on default_body | #fbc2ba | #57595e | 4.52 | 4.5 | pass |
| pewter | text on default_body | #f5f4f1 | #57595e | 6.37 | 4.5 | pass |
| pewter | text_dim on default_body | #d6d4cf | #57595e | 4.73 | 4.5 | pass |
| pewter | text_faint on default_body | #bdbbb6 | #57595e | 3.65 | 3.0 | pass |
| pewter | title_text on default_header | #ffffff | #64676c | 5.68 | 4.5 | pass |
| pewter | field_border vs field (UI) | #9a9ca1 | #3a3c40 | 4.02 | 3.0 | pass |
| pewter | field_border vs panel (UI) | #9a9ca1 | #43454a | 3.49 | 3.0 | pass |
| pewter | accent vs canvas (UI) | #94c2f7 | #4b4d52 | 4.56 | 3.0 | pass |
| pewter | state_done vs canvas (UI) | #82d290 | #4b4d52 | 4.66 | 3.0 | pass |
| pewter | state_error vs canvas (UI) | #fbc2ba | #4b4d52 | 5.45 | 3.0 | pass |
| pewter | state_running vs canvas (UI) | #94c2f7 | #4b4d52 | 4.56 | 3.0 | pass |
| pewter | state_queued vs canvas (UI) | #a1a6ae | #4b4d52 | 3.46 | 3.0 | pass |
| pewter | highlight vs canvas (UI) | #fff3b0 | #4b4d52 | 7.53 | 1.8 | pass |
| pewter | wire vs canvas (UI) | #b4b2ad | #4b4d52 | 3.99 | 3.0 | pass |
| pewter | pin_default vs default_body (UI) | #b4b2ad | #57595e | 3.31 | 3.0 | pass |
| pewter | waveform vs waveform_bg (UI) | #62d6c3 | #2f3134 | 7.41 | 4.5 | pass |
| pewter | playhead vs waveform_bg (UI) | #fff3b0 | #2f3134 | 11.62 | 4.5 | pass |
| pewter | playhead vs waveform (UI) | #fff3b0 | #62d6c3 | 1.57 | 1.3 | pass |
| pewter | chart vs panel (UI) | #94c2f7 | #43454a | 5.17 | 3.0 | pass |
| pewter | title_text on worst fruit header | #ffffff | (14 fruits) | 5.09 | 4.5 | pass |
| pewter | text_dim on worst fruit header | #d6d4cf | (14 fruits) | 3.44 | 3.0 | pass |
| pewter | text on worst fruit body | #f5f4f1 | (14 fruits) | 6.16 | 4.5 | pass |
| pewter | text_faint on worst fruit body | #bdbbb6 | (14 fruits) | 3.53 | 3.0 | pass |
| dusk | text on panel | #eceae6 | #2a2c31 | 11.63 | 4.5 | pass |
| dusk | text_dim on panel | #bab7b1 | #2a2c31 | 6.98 | 4.5 | pass |
| dusk | text_faint on panel | #8d8b86 | #2a2c31 | 4.10 | 3.0 | pass |
| dusk | text on section | #eceae6 | #31343a | 10.39 | 4.5 | pass |
| dusk | text_dim on section | #bab7b1 | #31343a | 6.24 | 4.5 | pass |
| dusk | text on field | #eceae6 | #232529 | 12.78 | 4.5 | pass |
| dusk | text_dim on field | #bab7b1 | #232529 | 7.67 | 4.5 | pass |
| dusk | text on canvas | #eceae6 | #202226 | 13.26 | 4.5 | pass |
| dusk | text_dim on canvas | #bab7b1 | #202226 | 7.96 | 4.5 | pass |
| dusk | text on w_inactive | #eceae6 | #363940 | 9.62 | 4.5 | pass |
| dusk | text on w_hovered | #eceae6 | #41454c | 8.02 | 4.5 | pass |
| dusk | text on w_active | #eceae6 | #4c5058 | 6.73 | 4.5 | pass |
| dusk | text on banner | #eceae6 | #6b2e28 | 8.54 | 4.5 | pass |
| dusk | accent on panel | #6c9fe0 | #2a2c31 | 5.11 | 4.5 | pass |
| dusk | error on panel | #ec7d70 | #2a2c31 | 5.15 | 4.5 | pass |
| dusk | warning on panel | #ddae55 | #2a2c31 | 6.83 | 3.0 | pass |
| dusk | error on default_body | #ec7d70 | #2e3136 | 4.81 | 4.5 | pass |
| dusk | text on default_body | #eceae6 | #2e3136 | 10.86 | 4.5 | pass |
| dusk | text_dim on default_body | #bab7b1 | #2e3136 | 6.52 | 4.5 | pass |
| dusk | text_faint on default_body | #8d8b86 | #2e3136 | 3.84 | 3.0 | pass |
| dusk | title_text on default_header | #f4f2ee | #3d4047 | 9.29 | 4.5 | pass |
| dusk | field_border vs field (UI) | #7a7e87 | #232529 | 3.77 | 3.0 | pass |
| dusk | field_border vs panel (UI) | #7a7e87 | #2a2c31 | 3.43 | 3.0 | pass |
| dusk | accent vs canvas (UI) | #6c9fe0 | #202226 | 5.82 | 3.0 | pass |
| dusk | state_done vs canvas (UI) | #68ba77 | #202226 | 6.74 | 3.0 | pass |
| dusk | state_error vs canvas (UI) | #ec7d70 | #202226 | 5.87 | 3.0 | pass |
| dusk | state_running vs canvas (UI) | #6c9fe0 | #202226 | 5.82 | 3.0 | pass |
| dusk | state_queued vs canvas (UI) | #7a808a | #202226 | 4.01 | 3.0 | pass |
| dusk | highlight vs canvas (UI) | #ecd65a | #202226 | 10.87 | 1.8 | pass |
| dusk | wire vs canvas (UI) | #8d8b86 | #202226 | 4.68 | 3.0 | pass |
| dusk | pin_default vs default_body (UI) | #a3a19c | #2e3136 | 5.06 | 3.0 | pass |
| dusk | waveform vs waveform_bg (UI) | #45c2ae | #17181b | 8.10 | 4.5 | pass |
| dusk | playhead vs waveform_bg (UI) | #ecd65a | #17181b | 12.12 | 4.5 | pass |
| dusk | playhead vs waveform (UI) | #ecd65a | #45c2ae | 1.50 | 1.3 | pass |
| dusk | chart vs panel (UI) | #6c9fe0 | #2a2c31 | 5.11 | 3.0 | pass |
| dusk | title_text on worst fruit header | #f4f2ee | (14 fruits) | 6.11 | 4.5 | pass |
| dusk | text_dim on worst fruit header | #bab7b1 | (14 fruits) | 3.42 | 3.0 | pass |
| dusk | text on worst fruit body | #eceae6 | (14 fruits) | 10.18 | 4.5 | pass |
| dusk | text_faint on worst fruit body | #8d8b86 | (14 fruits) | 3.59 | 3.0 | pass |

`highlight` is checked at 1.8:1 rather than 3:1. It's drawn as a glow around a node that also has its run-state ring, so it never carries meaning alone. On Paper a yellow can't reach 3:1 on a light ground without turning brown.

The unit tests in `theme.rs` (`themes_are_named_and_readable`, `waveforms_and_spectrograms_are_readable`) would carry these rules over. In particular, the header test becomes 4.5:1 for `header_text` on every fruit and 3:1 for `text_dim` on every fruit, and a test that the closest pair of family fruits is at least 4 apart in OKLab×100 would join them.

Swatch files: `claude-debug/themes/swatch-{paper,pewter,dusk}.{svg,png}`. The current look is in `claude-debug/themes/current-{medium,dark}.png`, and the palettes as JSON are in `claude-debug/themes/palettes.json`. The swatches were rendered with `rsvg-convert` in the system sans, since Inter isn't installed system-wide. Sizes and colours are exact, but the letterforms aren't Inter.

## 8. Open questions

1. **Default theme.** Should Paper (light) be the default, or Pewter, keeping a grey for image-heavy work? Paper is proposed because every flow tool surveyed is light and the complaint was darkness.
2. **Keep Medium and Dark as they are**, renamed "Classic", for anyone who likes them? Or map old prefs to Pewter and Dusk?
3. **Engine colours.** Should they keep only their hue in each theme's register (proposed), snap to the nearest fruit, or keep the exact hex?
4. **Hash with probe.** It means adding a family to an engine can recolour the one family it clashed with. Is that acceptable, or should colours be a strict function of the id (accepting about one clash per eight families)?
5. **More fonts to bundle.** Inter SemiBold, plus optionally IBM Plex Sans and Atkinson Hyperlegible Next, cost about 0.3 to 0.6 MB each.

## Sources

- Node-RED editor colours: [colors.scss](https://github.com/node-red/node-red/blob/master/packages/node_modules/@node-red/editor-client/src/sass/colors.scss); node colours from the core node `.html` files (for example [20-inject.html](https://github.com/node-red/node-red/blob/master/packages/node_modules/@node-red/nodes/core/common/20-inject.html), [10-function.html](https://github.com/node-red/node-red/blob/master/packages/node_modules/@node-red/nodes/core/function/10-function.html)).
- Blender default theme: [userdef_default_theme.c](https://github.com/blender/blender/blob/main/release/datafiles/userdef/userdef_default_theme.c).
- ComfyUI palettes: [src/assets/palettes](https://github.com/Comfy-Org/ComfyUI_frontend/tree/main/src/assets/palettes); LiteGraph node colours in [LGraphCanvas.ts](https://github.com/Comfy-Org/litegraph.js/blob/main/src/LGraphCanvas.ts).
- KNIME node colours: [KNIME forum, "Why nodes are different colors?"](https://forum.knime.com/t/why-nodes-are-different-colors/40800).
- Ab Initio GDE: [Ab Initio overview (training deck)](https://www.slideshare.net/SandeepSharma65/ab-initio-training-abinitio-architecture), [GDE description](https://www.liquisearch.com/ab_initio_company/the_graphical_development_environment).
- Nuke node colours: [Nukepedia NodeColors](https://www.nukepedia.com/tools/miscellaneous/nodecolors/), [Nuke Stage preferences](https://learn.foundry.com/nuke-stage/1.1/Content/misc/preferences_studio.html).
- LabVIEW, Max/MSP, Pure Data, Unreal, Houdini, TouchDesigner, NiFi, Orange, Enso, n8n and Alteryx are described from their documentation and product screenshots. Their values are approximate.
- Contrast: WCAG 2.2, success criteria 1.4.3 (text, 4.5:1) and 1.4.11 (non-text, 3:1). OKLCH: Björn Ottosson, "A perceptual color space for image processing" (2020).
