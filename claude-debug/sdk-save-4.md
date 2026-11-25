# Save Result, fourth part: the label font is Aileron (B125)

Branch `feat/sdk-save-4`, started from `feat/sdk-save-3` (7041678), uncommitted, 2026-11-22.

## What was done

- **The glyph table.** `claude-debug/sdk-save/make_sheet_font_aileron.py` calls `ImageFont.load_default(size=11)`. It stops unless the result is a `FreeTypeFont`, so it can't silently use the bitmap fallback. It ran with Pillow 13.0.0 and FreeType 2.14.3, and both versions are written into the table's header.
- **Size 11.** The line is 14 rows (ascent 11, descent 3). It sits 1 pixel under the cell, inside the existing 15-pixel band, so the layout is unchanged. Size 12 would need a 16-pixel band; size 10 is small.
- **What each glyph stores.** Its 8-bit coverage over the line's 14 rows, the column where the coverage starts relative to the pen, its width, and its advance rounded to whole pixels.
- **What was included (102 glyphs).** All of printable ASCII, plus the seven Latin-1 signs that Pillow's Aileron has: `© « ° ± ´ · »`. Pillow's "more limited character set" has no accented letters, so `é` draws as `?`. A glyph counted as missing when it drew the same as U+FFFF, the font's .notdef box.
- **The data file.** `sheet_font.json` is byte-identical in `crates/engine/src/` and `python/src/plumcraft_engine/`. Its `about` and `source` fields name the source: Aileron Regular, Sora Sagano, dotcolon, CC0 1.0, via Pillow 13.0.0 `load_default(size=11)` with FreeType 2.14.3.
- **Drawing (both SDKs).** The pen starts at an integer position and there is no kerning. Each covered pixel becomes `(ink·a + old·(255−a) + 127) / 255` per channel, worked in integers, one glyph after another. Labels are cut to the characters whose advances fit in the cell and centred by their total advance. The interface (spec 15) states this rule.
- **The fixture.** `crates/engine/tests/fixtures/sheet.json` has the new SHA-256 `869f49f2…f527094`, with shape [566, 536, 3] unchanged. Rust and Python each produce exactly that hash.
- **Tests.** The font test is now `the_sheet_font_is_aileron_and_the_pythons_copy`. The label checks in both SDKs' integration tests now look for dark pixels instead of exact ink, because edges are antialiased.
- **Docs.**
  - THIRD-PARTY.md: the Aileron entry (CC0, the URL, how the table was made, the name table) and a statement that no courB08 data remains.
  - `docs/interface.md`: section 15.
  - `docs/BUGS.md`: B125 moved to Fixed.
  - Note 24: "The label font: Aileron".
  - `docs/STATUS.md`.

## The embedded TTF's name table

Read with `claude-debug/sdk-save/aileron_probe.py`, from the 12,676-byte font embedded in Pillow 13.0.0's `ImageFont.load_default`. The Mac and Windows records say the same.

| ID | Field | Value |
|---|---|---|
| 0 | Copyright | No Rights Reserved. |
| 1 | Family | Aileron |
| 2 | Style | Regular |
| 3 | Unique ID | 1.102;UKWN;Aileron-Regular |
| 5 | Version | Version 1.102;PS 001.102;hotconv 1.0.70;makeotf.lib2.5.58329 |
| 6 | PostScript name | Aileron-Regular |
| 8, 9 | Vendor, designer | Sora Sagano |
| 11, 12 | Vendor URL, designer URL | http://www.dotcolon.net |

There is no licence entry (ID 13) and no licence URL entry (ID 14).

## Who checked what (recorded in B125)

- The controller, 2026-11-22: dotcolon's statement ("Aileron is No Rights Reserved", linked to CC0 1.0) and Pillow's docstring.
- The agent, 2026-11-22: the name table above.

## The rendered sheet

`sheet-aileron.png`, in the session's scratchpad folder (the full path is in the hand-back), is the fixture's sheet. Enlarged crops of its labels are `sheet-aileron-labels.png` and `sheet-aileron-label3.png` in the same folder. "dim 1", "x=2.5" and "Sweep: 3 ?" read cleanly, with antialiased edges, centred under their cells.

## Gate

Run on 2026-11-22 with `claude-debug/sdk-save/gate.sh`. `origin/feat/native-n0` has only PR #170's merge past 7041678, which changes no files, so nothing needed merging.

| Step | Result |
|---|---|
| fmt, clippy (`-D warnings`), schema | clean |
| nextest | 1,645 of 1,646 passed. The one failure is the known `params_coverage hidden_params_match_the_baseline`, which also failed before this branch |
| hash gates, core toolkit | pass |
| pytest, ruff | 249 passed; all checks passed |
| private strings | no match in 1,307 tracked files; the new files are clean |

Also recorded: in note 24's decision log, the maintainer's answer to "B99: watch or investigate?" was, verbatim, "watch".

## Not done

- AI.md still lists B125 as open. AI.md is updated when the commit message is written.
- `native/` is untouched and the shells need nothing for this: the sheet is drawn in the SDKs.
