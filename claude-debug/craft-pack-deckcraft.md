# DeckCraft pack (worker C, 2026-10-28)

## Status

**Done.** `crates/craft/packs/deckcraft.toml` (26 curated nodes plus `dk.run`), `packs/deckcraft-demo.json`, `crates/craft/tests/pack_deckcraft.rs` (2 tests, both pass against DeckCraft 0.12.0, 330 commands). Editor screenshot: left to the coordinator.

## How it drives DeckCraft

| Manifest part | DeckCraft |
| --- | --- |
| server | `deckcraft-cli mcp --automation-read-root … --automation-write-root …` |
| list / run | `list_commands`; `run_command {command, params}` |
| open | tool `open_presentation {path}` (.deckcraft, .pptx) |
| save | tool `save_presentation {path}`; native `.deckcraft` (zip), byte-stable (the test checks two runs give one handle) |
| close_all | command `file.close` (round 2: when it fails because nothing is open, the adapter replaces the process; this replaced the round-1 `batch` of four `file.close` with keepGoing) |
| export | command `file.export {path, format}` (png, pdf, pptx, gif, outline) |
| selection | not object selection: `all`/`none` = `slide.first`, `set` = `edit.select` (see below) |
| volatile | `path`, `index`, `document` (the presentation's index in the process) |
| gui | `deckcraft <file>` (`PLUMCRAFT_DECKCRAFT_APP` overrides) |

## Nodes

| Family | Nodes |
| --- | --- |
| Presentation | dk.open, dk.new (theme or starter template from the `file.templates` catalogue), dk.properties, dk.inspect, dk.info |
| Slides | dk.add_slide (layout, title, body, `at`), dk.from_outline (markdown outline → slides), dk.delete_slide, dk.duplicate_slide, dk.move_slide, dk.hide_slide, dk.slide_layout, dk.notes, dk.transition |
| Text | dk.replace (text.replace on every slide), dk.replace_font |
| Design | dk.theme, dk.variant, dk.background (all slides), dk.header_footer, dk.slide_size |
| Export | dk.export_png (IMAGE, one slide, scale), dk.export_pdf (PDF; slides/notes/handouts), dk.export_pptx (PPTX), dk.export_gif (GIF, one frame per slide), dk.export_outline (TXT) |
| Advanced | dk.run (slide., shape., format., text., design., transition., animation., table., chart., smartArt., arrange., section., comment., master., guide., selection, in-document inserts) |

PPTX, GIF and TXT are viewer-less wire types.

Demo: New (Harbor) → Slides from outline → Replace text → Transition (fade, slide 1) → Header and footer → Export PNG (slide 1), Export PDF, Presentation info.

Test: parses; live, the catalogue lists exactly the curated nodes; the demo runs (2 outline slides, 1 replacement, 3 slides, a 960×540 PNG, a PDF); a second run gives the same document handle; new → outline → delete slide 0 → outline text starts "Alpha"; every doc-taking node runs with its defaults on the Pitch template.

## Assumptions

- Commands act on the current slide unless they take an index; a warm process keeps the current slide from the last node, a cold open starts at slide 0. Curated nodes take explicit indices. `slide.fromOutline` inserts after the current slide, so dk.from_outline has a `select` port whose `all` is `slide.first`: the outline always goes after the first slide. This is a deliberate misuse of `[selection]`; the select port is advanced and its `ids:`/`color:` forms are meaningless for this node.
- Theme, colour-scheme and transition names are hard-coded from `design.themes` and `transition.list` (both need an open presentation, and the catalogue reader runs without one). Templates come from `file.templates` (works without a document).
- `dk.new`: `theme` and `template` are both optional; DeckCraft refuses both together.
- `dk.replace` fails (config) when nothing matches; that is DeckCraft's behaviour (`text.replace` "no matches").

## Left out, and why

- `design.colors {name}` and `design.fonts {name}`: dropped by the adapter's params check, which reads only the first alternative of `{} opens … | {name} | {colors: …}`. Adapter proposal: `grammar::accepted` should take the union of keys over `|` alternatives.
- Shape formatting (format.*, shape.fill …) as curated nodes: needs shape ids on the current slide; available through dk.run with `select` ids.
- file.save/saveAs/saveBytes/export (as a command), show.* (slide show), media.*, insert.picture/audio/video/photoAlbum and slide.reuse (read files), design.browseTheme, design.ideas, recovery, review.compare.
- No `[thumb]` (needs `artboards[0]` size); proposal as in the WordCraft notes. Exports of PNG get their own thumb.

## Open problems and proposals

1. Adapter: per-node `before = ["slide.first"]` commands, instead of borrowing `[selection]`.
2. (Resolved in round 2: a failed `close_all` now respawns the process.)
3. Adapter grammar: union of keys over `|` alternatives (above). Still open after round 2, so dk.colors and dk.fonts remain left out as a follow-up.
