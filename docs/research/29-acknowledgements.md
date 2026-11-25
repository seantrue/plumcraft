# 29. Acknowledgements: which licences need display, and how the list is made

FR4. The maintainer: "Add list of licenses that need display on Help/About dialog", then, asked whose and where: "All licences, in a separate Acknowledgements window". This note is part 1: what must be shown, where the list comes from, and what needs the maintainer's word before anything is built.

## 1. Summary

- **Almost everything is permissive and needs only its notice shown.** The Mac and iPad core library (`plumcraft-ffi` for `aarch64-apple-darwin`, the text, array and audio engines compiled in) pulls 207 third-party crates. Every one declares a licence; none is unknown. 191 are MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0 or a choice among them.
- **The list can be made offline from what the build already has.** `cargo metadata --offline --locked` gives each crate's licence expression and folder, and 202 of the 207 shipped crates carry their licence texts in the local registry. The other 5 declare plain MIT, Apache-2.0, BSD-3-Clause or Zlib, whose standard texts PlumCraft would keep. No new tool is needed and no network step. `cargo-about`, `cargo-deny` and `cargo-license` are not installed here.
- **Four things need the maintainer's decision before part 2** (section 5):
  1. the **Windows app's Microsoft packages** are under Microsoft's own terms, not an open-source licence;
  2. **Symphonia**, the audio decoders, is **MPL-2.0**, a file-level copyleft;
  3. the **Python engines' wheels** bundle native libraries, some under the GPL with an exception;
  4. the egui editor ships **fonts under the OFL and the Ubuntu Font Licence**.

  None of these makes distribution impossible as far as this note can tell. But they are terms to accept or conditions to meet, which the brief says not to decide here. So this note stops before part 2, as asked.

## 2. What must be displayed

What each licence asks of a binary, in its own terms:

| Licence | Crates (the shipped Mac library unless said) | What a binary must carry |
|---|---|---|
| MIT (alone, or chosen from MIT OR Apache-2.0) | 154 (with the "or" forms) | The copyright notice and the permission notice |
| Apache-2.0 (or chosen) | in the "or" forms; 1 alone | A copy of the licence, and the crate's NOTICE file when it has one |
| BSD-2-Clause, BSD-3-Clause, 0BSD | about 10, alone or in "or" forms | The copyright notice, the conditions and the disclaimer (0BSD: nothing) |
| ISC | (none in the Mac library; 1 elsewhere in the tree) | The copyright and permission notice |
| Zlib | 3 alone, 9 in "or" forms | Nothing in a binary (asked only of altered source); listing it is courtesy |
| Unicode-3.0 | 5 (the `icu_*` data) | The copyright and permission notice, in the documentation |
| CC0-1.0, Unlicense, MIT-0 | in "or" forms | Nothing |
| BSL-1.0 | 2, in the Windows build only (`clipboard-win`, `error-code`) | Nothing in a binary-only distribution |
| MPL-2.0 | 16 (`symphonia` and its codecs) | The licence, and where the source of the MPL files is (section 5.2) |
| OFL-1.1 | the egui shell's default fonts; the array engine's Inter (`crates/array/assets/fonts`) | The copyright notice and the licence, with the fonts |
| Ubuntu Font Licence 1.0 | the egui shell's default fonts (`epaint_default_fonts`) | The copyright notice and the licence, with the fonts |

**Where an expression offers a choice** (`MIT OR Apache-2.0`), the list would name the choice PlumCraft takes: MIT where offered, since it asks the least. For one crate the choice matters: `self_cell` (`Apache-2.0 OR GPL-2.0-only`) is taken as Apache-2.0. For another, `r-efi` (`MIT OR Apache-2.0 OR LGPL-2.1-or-later`), it is taken as MIT; that crate is in the lockfile only for UEFI targets, not in any shipped binary.

**The code adapted into PlumCraft's own source** (`THIRD-PARTY.md`: SolidCraft's STL reader and rasteriser under MIT, the contact sheet's Aileron font under CC0) goes in the list the same way, from that file. **PlumCraft's own licence** (MIT OR Apache-2.0) heads the list.

## 3. How the list is produced

### 3.1 Rust crates (the core library, the egui editor, each Rust engine)

A small generator, run by hand and checked by a test, writes the list as data checked into the repository:

1. It reads `cargo metadata --format-version 1 --offline --locked` for the binary's package and target (the Mac and iPad library, the egui editor, the Windows core DLL). With `--filter-platform` and the resolved graph, it keeps only normal (not dev or build) dependencies.
2. For each third-party crate it reads the licence expression and the licence files in its folder (`LICENSE*`, `COPYING*`, `NOTICE*`, `UNLICENSE*`), which the local registry already holds.
3. A crate that ships none gets the standard text of its licence from a small set PlumCraft keeps (MIT, Apache-2.0, BSD-2/3-Clause, Zlib, ISC). Today that applies to 5 crates.
4. It groups crates by licence. Each distinct licence text appears once, with the crates that use it; a copyright line that differs per crate stays with its crate.

A test fails when the checked-in list no longer matches the lockfile (a crate added, removed or upgraded), as the shell schema's does today. The generator needs no network and no tool beyond cargo. The brief offered this route ("`cargo metadata` plus the crates' licence files … a checked-in generated file with a test that fails when it is stale is acceptable"). Since none of the dedicated tools is installed, there is no fork here: the generator is the one way that works offline.

### 3.2 Swift packages

The Apple shells use no third-party Swift package (`native/apple/*/project.yml` names only PlumCraft's own `PlumCraftFFI`). Their list is the core library's (3.1) and Apple's own frameworks, which need no notice in an app.

### 3.3 The Windows app's NuGet packages

`PlumCraft.App` references `Microsoft.WindowsAppSDK` 2.6.5 and `Microsoft.Graphics.Win2D` 1.4.0. The app is self-contained, so it redistributes both. Read from the test VM's NuGet cache:

- **Windows App SDK:** its package's licence is a file, "MICROSOFT SOFTWARE LICENSE TERMS — MICROSOFT WINDOWS APP SDK", with a `NOTICE.txt` of the open-source components inside it.
- **Win2D:** names only a Microsoft EULA URL (`eula_win2d_10012014.htm`), with no file in the package.

The test projects' packages (xunit and the test SDK) are not shipped. The list for Windows would hold the core DLL's crates (3.1) plus these two, generated the same way from the packages' files.

### 3.4 Python engines

An engine on the Python SDK can list the installed distributions it imports, from their metadata (`importlib.metadata`: the `License-Expression` or `License` field and the files under `*.dist-info/licenses/`). The SDK's own dependencies are:
- numpy 2.5.4 (BSD-3-Clause AND 0BSD AND MIT AND Zlib AND CC0-1.0, 20 licence files);
- Pillow 13.0.0 (MIT-CMU);
- pyuncommonkit 0.3.0 (MIT).

### 3.5 Engines and an application's own engine

Each engine supplies its own list through the interface (part 2: an additive capability, so an engine without it shows "no list given" under its name). An engine compiled into an application goes through the same path, so nothing of an application's list is in PlumCraft's public code.

## 4. The window (for part 2)

The list is data the core holds and serves (`app.acknowledgements`, a command beside About in the menu model). It is grouped by licence, each licence's text once, the packages under it, searchable or at least sectioned, with selectable text. On the Mac a window of its own; on the iPad and iPhone a sheet from About; on Windows a window; in egui a window.

## 5. For the maintainer

1. **The Windows App SDK and Win2D are under Microsoft's terms, not an open-source licence.** The app redistributes both. Showing their terms in the list is easy. Whether PlumCraft meets their conditions for redistributing Microsoft's code is a reading of those terms this note does not make. Choices:
   - **A.** list them with their full terms and the NOTICE file, and go on;
   - **B.** list them, and have the terms read before the next Windows release;
   - **C.** leave the Windows app's list to a later slice.
2. **Symphonia (MPL-2.0) is in the core library**, through the audio engine compiled in. For an unmodified MPL library, the obligation is to say where its source is and include the licence. The list would do both, giving the crate's repository and version. Modifying a Symphonia file would bring that file's source under the MPL. Choices:
   - **A.** accept it, the list carrying the licence and the source location;
   - **B.** keep the audio engine out of the core library.
3. **The Python engines' wheels bundle native libraries.**
   - **numpy:** its licence files name OpenBLAS, LAPACK and the GCC runtime (`libgfortran`, under the GPL with the GCC Runtime Library Exception).
   - **Pillow:** its licence file names FreeType (FTL or GPL-2.0), libjpeg, zlib, libtiff, HarfBuzz, Brotli, libwebp, lcms, OpenJPEG and libavif.

   The exceptions and dual licences generally allow this use, but they are copyleft texts. Choices:
   - **A.** an engine lists its wheels' licence files in full, as installed;
   - **B.** list the wheel's declared licence only, linking to its files.
4. **Fonts.**
   - **egui:** the editor embeds egui's default fonts (OFL-1.1 and the Ubuntu Font Licence).
   - **Array engine:** embeds Inter (OFL-1.1, `crates/array/assets/fonts/OFL-Inter.txt`).

   Both licences permit embedding and ask for the notice with the fonts, which the list would carry. Nothing to decide unless the maintainer wants these fonts replaced.

Recommendation: 1A, 2A, 3A, 4 as is. Part 2 then builds the list, the capability and the windows as in sections 3 and 4.
