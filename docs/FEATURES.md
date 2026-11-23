# Features

Features requested in passing (`feature: …`), kept until built or declined. Numbers are never reused. Entries with an issue are also tracked on GitHub.

Entries are numbered `FR<n>`, not `F<n>`: in this project `F1` to `F85` already name the floor rows of `docs/research/24-native-shells.md`.

## Open

## Done

### FR1. Promote a curve editor (and other in-place editors) to a group input

- Requested: 2026-11-23
- Request: "curve ui needs a way to promote it to a group input. This will be true for other nodes that allow masked editing, etc"
- Context: the maintainer's hand use of the release build of Plummer 0.7.0 on their iPad and iPhone (dev branch at `2c3de06`). Groups already promote ordinary inputs (`groups` in the control protocol lists each group's promoted inputs); a node's own editor (the curve editor, a mask editor) has no way onto the group's face.
- Design (the maintainer, 2026-11-23): asked "What does the group show for a promoted editor? The editor itself on the group's face, or a row that opens it? I'd lean towards a row that opens it, so a group with several promoted editors stays compact, but the curve may be worth showing in place." and "Which editors count? Every value an engine names an editor for, or a specific list? I'd say every one, so it works for editors that don't exist yet", they answered: "Fr1-yes,yes". So: the group shows a row that opens the editor, and every value an engine names an editor for can be promoted.
- When (the maintainer, 2026-11-23): "After the others." (after the fixes for B133, B134 and B135).
- Issue: none
- Done: 2026-11-23, branch `feat/fr1-group-editors`: a promoted value with an editor shows on the group's face as a row that opens it. On the native shells, which have no editors of their own yet, it opens the core's value window (the value as JSON).

## Declined
