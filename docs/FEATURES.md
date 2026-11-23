# Features

Features requested in passing (`feature: …`), kept until built or declined. Numbers are never reused. Entries with an issue are also tracked on GitHub.

Entries are numbered `FR<n>`, not `F<n>`: in this project `F1` to `F85` already name the floor rows of `docs/research/24-native-shells.md`.

## Open

### FR1. Promote a curve editor (and other in-place editors) to a group input

- Requested: 2026-11-23
- Request: "curve ui needs a way to promote it to a group input. This will be true for other nodes that allow masked editing, etc"
- Context: the maintainer's hand use of the release build of Plummer 0.7.0 on their iPad and iPhone (dev branch at `2c3de06`). Groups already promote ordinary inputs (`groups` in the control protocol lists each group's promoted inputs); a node's own editor (the curve editor, a mask editor) has no way onto the group's face.
- Open question: what does the group show for a promoted editor: the editor itself on the group's face, or a row that opens it?
- Open question: which editors count, beyond the curve: every value an engine names an editor for (`editor` on a port), or a named list?
- Issue: none

## Done

## Declined
