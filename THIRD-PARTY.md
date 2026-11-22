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

## The contact sheet's label font (licence NOT verified: a release blocker, B125)

`crates/engine/src/sheet_font.json` and `python/src/plumcraft_engine/sheet_font.json` (identical)
hold the glyphs the engine SDKs draw contact sheet labels with: printable ASCII, 6 by 11 pixels,
one bit a pixel. They were extracted once, on 2026-11-22, from **Pillow's built-in default bitmap
font** (`PIL.ImageFont.load_default_imagefont()`, Pillow 13.0.0) by a scratch script,
`make_sheet_font.py`, which drew each character with that font and kept its pixels.

**Where those glyphs come from upstream, and under what terms, is NOT yet verified.** The
controller believes Pillow's default bitmap font is Adobe's Courier bitmap from the X11
distribution, under Adobe's own notice terms rather than Pillow's licence; that is unverified.
The maintainer (2026-11-22): "Just use the Pillow font. We will deal with issues before Plummer is
released to public." Verify its licence, or replace it, before a public release (`docs/BUGS.md`
B125).
