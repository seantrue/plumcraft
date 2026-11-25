# Photos as a source for Browse (iPad and iPhone)

*2026-11-23, branch `feat/path-photos` from `fix/b133-path-browse`, brought up to `d29630c`. Uncommitted. No device.*

## Answer first

When a path takes pictures only, Browse on the iPad and iPhone now offers **Photos** as well as **Files**:

1. An action sheet, "Choose the picture from: Photos, Files, Cancel".
2. For Photos, the photo library's picker, which shows images only and needs no permission.
3. The photo is copied into the app's Documents › Picked Files. It is kept as it is when its form is one of the path's kinds. Otherwise it is converted to the PNG or JPEG the path names; if the path names neither, the ask is cancelled with a message.
4. The copy's path is the answer, set on the node as typing sets it.

The port's filter is honoured twice: the choice appears only for picture-only kinds, and the kept file is in a form the kinds name.

**The design adds nothing to the contract, but it is the maintainer's call (below).** It has been built and unit-tested, but not launched through to a pick. No engine offers a path that takes pictures only yet, so no UI test can reach the choice.

## Golden rule 4 and "no shell-side user action"

I reread my earlier note that choosing Photos or Files is part of answering one `file.open` ask and adds no core command. I checked it against both rules:

- **The core still holds the action.** The person's action is Browse, which is the core's `row.browse`, and the core raises one `file.open` (purpose `path`). The sheet only picks which platform picker answers that ask, as the document picker's own sidebar does. The Mac's open panel already lists Photos in its sidebar, and nobody would call that a shell-side action.
- **How the core knows:** it receives one answer, a path. Nothing tells it which source gave the path, and nothing it does depends on that.
- **How a script drives it:** a script answers the ask with a path, as for any `file.open`. A script never drives the shell's pickers, Files' included.

This is written in `docs/shell-interface.md` 11.3 as **proposed, not decided**, together with the alternative for the maintainer:

- **Alternative.** The core decides that Photos is offered and says so in the ask (`sources: ["files", "photos"]`), and an answer may say which source gave it (`source`). A script could then ask a shell for its Photos picker. That is an addition to the contract, a capability.

## What was built

- `PlumCraftShell/PickedFiles.swift`:
  - `picturesOnly(extensions)`: png, jpg, jpeg, heic, heif, gif, tif, tiff, webp, bmp, and at least one.
  - `photoName(suggested:ext:at:)`: the library's suggested name, else "Photo 2026-11-23 at 09.05.07", with the kept form's extension.
- `PlumCraftPad/App/Asks.swift`:
  - `chooseSource`: the action sheet, `path-source`.
  - `photos`: a `PHPickerViewController` with the filter `.images`, one photo, or several when the path takes several.
  - `keepPhoto`: the provider's file in a form the kinds take, else a PNG or JPEG made from the image.
  - The document picker is unchanged, moved into `files`.
  - The status's `picking` gains `source` (`choice`, `photos`, `files`) for scripts and tests.
- `docs/shell-interface.md` 11.3, as above. `docs/BUGS.md`: B133 stays Open, with a Built line for this.

## Tests

| Check | Result |
|---|---|
| `swift test` (the new `PhotosAsASource` suite: when Photos is offered, how a photo is named) | passed, 21 tests in the shell's suites |
| The iPad app built and `testBrowseForAPath` run on the simulator (a text path: Files only, as before) | see the controller's report |

## Next

- A test engine node whose path takes pictures (for example `ports.read_picture`, png and jpg). The UI test could then reach the choice, open Photos, pick one of the simulator's sample photos and check the path written into the node. Adding a node to the test engine changes the conformance cases' kind counts, so I left it to a decision.
- Windows needs nothing for this (its file dialog shows Pictures). Android, when it comes, has the same question.
