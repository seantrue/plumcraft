# Photos for Browse, decided and launched (11A, 12A)

*2026-11-23, branch `feat/path-photos`, the dev branch `c0ff5cb` merged in (uncommitted: the merge and the changes below). No device.*

## Answer first

The Photos path is launched end to end on the iPad simulator by a new UI test, `testBrowseOffersPhotosForAPicture` (`PlumCraftPad/UITests/PhotosTests.swift`, 18 s):

1. Browse on a picture path.
2. "Photos or Files".
3. Files: the document picker for the same ask, cancelled, with nothing set.
4. Browse again, then Photos: the photo library's picker.
5. A photo of the simulator's own library picked.
6. The node's path: `…/Documents/Picked Files/IMG_0111.jpeg`, which exists.

The photo picker runs in another process, so its first photo is tapped where it is drawn (about 0.44, 0.45 of the 834 by 1210 window). That place is the iPad's layout; the iPhone's isn't tried. It passed at the first place tried.

## The decisions (recorded in note 24, verbatim line included)

- **11A:** Photos is the shell's business only. The core asks for a file and never knows the source. `docs/shell-interface.md` 11.3 now says so as decided, and the `sources` and `source` alternative is recorded as declined.
- **12A:** the test engine has a node whose path takes pictures only.

## The test-engine node

`ports.picture`, "Read Picture File" (`crates/ports/src/lib.rs`):

- one path input, `file`, mode `open`, kinds PNG and JPEG (`png`, `jpg`, `jpeg`);
- one TEXT output: the file's size, "N bytes", which shows the picked file was there.

**Counts it moves: none found.**

- Nothing in the conformance cases, the Swift or C# runners, or the Windows person check counts the test engine's kinds. They count wires and commands, and the node is reached by its kind name.
- The Rust tests of the test engine and the editor core passed unchanged (220 of 220).
- The Mac and iPad runs are in the gate below.
- **Windows:** no file changes. The Windows agent should run its suite with the rebuilt test engine to confirm that no count moved there.

## Merge

The dev branch `c0ff5cb` was merged without committing. `AI.md` and `docs/BUGS.md` conflicted, and both sides were kept.

## Gate

`claude-debug/n4-decided-2/photos-gate.sh`, about 33 minutes. Linux is the controller's (Rust changed).

| Check | Took | Result |
|---|---|---|
| Rust gate | 204 s | 1662 of 1663: only the known `hidden_params_match_the_baseline` failed. `cargo fmt` flagged the new node's line, which was then fixed and checked clean |
| swift test | 46 s | passed (the new PhotosAsASource suite included) |
| Mac gate, twice (the second under load) | 676 s | 69 of 69 cases each; person check 59 passed, 0 failed (screen locked, so accessibility was only seen) |
| iPad standard: device build, whole suite, UI tests | 336 s | 60 of 60; UI 9 passed, 4 skipped (no test engine there), 0 failed |
| iPad whole suite with the test engine | 55 s | 68 of 68 |
| iPad, every UI test with the test engine | 291 s | 11 passed, 1 skipped (the iPhone-landscape test), 0 failed; `testBrowseOffersPhotosForAPicture` 17 s |

**Home listing.** During the iPad step a file appeared in the person's Library: `~/Library/Preferences/com.swapwizard.plummer.mac.scripted.plist`. It was rewritten at 14:28 while another agent's Mac person check was running from `.wt/plumcraft-packs` (started at 14:27). My Mac gate had ended at 14:19, so it is not this branch's. I left it as it is.
