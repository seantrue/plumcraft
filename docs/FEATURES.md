# Features

Features requested in passing (`feature: …`), kept until built or declined. Numbers are never reused. Entries with an issue are also tracked on GitHub.

Entries are numbered `FR<n>`, not `F<n>`: in this project `F1` to `F85` already name the floor rows of `docs/research/24-native-shells.md`.

## Open

## Done

### FR2. The default graph file extension becomes `.plum`; `.json` stays accepted

- Requested: 2026-11-24
- Request: "feat: change the default graph file extension from .json -> .plum \nContinue to accept .json as an alternate. Register .plum extension on platforms where it is not registered already"
- Context: said during the maintainer's hand use of an application built on the Mac shell. Graphs were saved as `.json` (New Graph on the iPad made "Untitled.json").
- Design (the maintainer, 2026-11-24): asked whether an existing `.json` graph keeps its name when saved, whether `.plum` gets a document icon, and whether applications built on the shell use `.plum` too, they answered: "Yes to all on FR2". So: an existing `.json` graph keeps its name, `.plum` is for new graphs and Save As, and applications built on the shell use `.plum` too.
- Icon (the maintainer, 2026-11-24): "Yes, draw a document icon".
- Issue: none
- Done (all but Windows): 2026-11-24, branch `feat/fr2-plum-extension`:
  - **The rule is the core's** (`crates/session/src/graph_file.rs`): default `plum`, accepted `plum` and `json`; the command `graph.fileTypes` gives it to the shells. Files are read by content, as before.
  - **Saving:** a new graph and Save As offer `.plum` first, `.json` still choosable; a graph opened from `.json` saves back to its `.json`; rename keeps the file's own extension; a bundle's graph may be either.
  - **Mac, iPad, iPhone:** the type `com.swapwizard.plummer.graph` (conforms to JSON) is declared and owned; JSON is an alternate, so Plummer does not claim every JSON file. New Graph on the iPad makes "Untitled.plum". egui follows the core's rule.
  - **The document icon:** a page with a folded corner carrying the app's mark, each size drawn separately (`native/apple/PlumCraftMac/App/PlumGraph.icns`; `assets/doc-icon/plum-document.ico` for Windows). iPad and iPhone declare none: Files there shows the app's icon or a thumbnail.
  - **Checked:** a shared conformance case (a new graph saves as `.plum`; a `.json` graph opens and saves back) on the core, the C ABI, egui and the launched Mac app; the Mac's own save panel read back offering `.plum`; UI tests on fresh iPad and iPhone simulators.
  - **Not checked:** a double-click in Finder (the test copy was deliberately not registered as a handler on the maintainer's Mac); pressing Save in the Mac's own panel; a device.
  - **Not done:** Windows (the file pickers follow the core by themselves; the file association, the icon and the usage text are listed in the pull request). egui's packaging registers no file type.

### FR1. Promote a curve editor (and other in-place editors) to a group input

- Requested: 2026-11-23
- Request: "curve ui needs a way to promote it to a group input. This will be true for other nodes that allow masked editing, etc"
- Context: the maintainer's hand use of the release build of Plummer 0.7.0 on their iPad and iPhone (dev branch at `2c3de06`). Groups already promote ordinary inputs (`groups` in the control protocol lists each group's promoted inputs); a node's own editor (the curve editor, a mask editor) has no way onto the group's face.
- Design (the maintainer, 2026-11-23): asked "What does the group show for a promoted editor? The editor itself on the group's face, or a row that opens it? I'd lean towards a row that opens it, so a group with several promoted editors stays compact, but the curve may be worth showing in place." and "Which editors count? Every value an engine names an editor for, or a specific list? I'd say every one, so it works for editors that don't exist yet", they answered: "Fr1-yes,yes". So: the group shows a row that opens the editor, and every value an engine names an editor for can be promoted.
- When (the maintainer, 2026-11-23): "After the others." (after the fixes for B133, B134 and B135).
- Issue: none
- Done: 2026-11-23, branch `feat/fr1-group-editors`: a promoted value with an editor shows on the group's face as a row that opens it. On the native shells, which have no editors of their own yet, it opens the core's value window (the value as JSON).

## Declined
