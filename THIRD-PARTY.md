# Third-party code

Code adapted from other projects into PlumCraft's own source, with its licence. Dependencies
pulled from crates.io carry their own licences and aren't listed here.

## SolidCraft

The STL reader (`crates/proto/src/mesh.rs`) and the mesh viewer's rasteriser
(`crates/ui/src/viewers/mesh.rs`) are adapted from SolidCraft's `read_stl`
(`crates/io/src/lib.rs`) and its CPU rasteriser (`crates/render/src/raster.rs`),
<https://github.com/tangency-labs/solidcraft>, which is dual-licensed MIT OR Apache-2.0. PlumCraft
uses it under the MIT licence:

```text
MIT License

Copyright (c) 2026 SolidCraft contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## The contact sheet's label font: Aileron Regular (CC0 1.0)

`crates/engine/src/sheet_font.json` and `python/src/plumcraft_engine/sheet_font.json` (identical)
hold the glyphs the engine SDKs draw contact sheet labels with. They are rasterised from
**Aileron Regular** by Sora Sagano (dotcolon), https://dotcolon.net/fonts/aileron, which dotcolon
publishes as "No Rights Reserved" under **CC0 1.0** (Creative Commons Zero, public domain
dedication). No notice is required; this entry is for provenance.

How the table was made (2026-11-22): a scratch script, `make_sheet_font_aileron.py`, called
Pillow's `ImageFont.load_default(size=11)` (Pillow 13.0.0, FreeType 2.14.3), which loads the
version of Aileron Regular that Pillow embeds, "with a more limited character set" (Pillow's
docstring). The script refused to run unless it got a FreeType font rather than Pillow's bitmap
fallback. It drew each character on its own and kept its 8-bit coverage over the line's height
(ascent 11, descent 3), the column the coverage starts at from the pen, and its advance rounded
to whole pixels. The table has printable ASCII and the seven Latin-1 signs that embedded Aileron
has (`© « ° ± ´ · »`): 102 glyphs. It has no accented letters; any character not in it is drawn
as `?`.

The embedded font's own `name` table says: copyright "No Rights Reserved.", family "Aileron",
style "Regular", version "Version 1.102", PostScript name "Aileron-Regular", designer "Sora
Sagano", vendor and designer URL "http://www.dotcolon.net". It has no licence or licence-URL
entry.

No data from Pillow's built-in bitmap font (courB08, Courier Bold 8 from X11), which an earlier
table was extracted from, remains in the repository.
