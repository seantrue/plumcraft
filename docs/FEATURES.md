# Features

Features requested in passing (`feature: …`), kept until built or declined. Numbers are never reused. Entries with an issue are also tracked on GitHub.

Entries are numbered `FR<n>`, not `F<n>`: in this project `F1` to `F85` already name the floor rows of `docs/research/24-native-shells.md`.

## Open

### FR4. A list of the licences that need display, in the Help/About dialog

- Reported: 2026-11-25
- Report: "Feat: Add list of licenses that need display on Help/About dialog"
- Context: said while testing an application built on the Mac shell, the day About was made to work on the Mac (B169). About today shows the build line, the core, shell-interface and C ABI versions and the engine.
- Design (the maintainer, 2026-11-25): "All licences, in a separate Acknowledgements window". So: the editor's own dependencies, each engine's, and an application's own engine's; shown in an Acknowledgements window of its own, reached from About.
- Issue: none

## Done

### FR5. Promoted labels carry the name of the node they came from, and can be renamed

- Reported: 2026-11-25
- Report: "feat: promoted labels should carry the name of the node they came from (perhaps as a section header), and should support renaming"
- Context: said while testing an application built on the Mac shell. Promoted here is taken to mean a parameter of a node inside a group shown on the group's own face (see FR1, promoting an in-place editor to a group input).
- Design (the maintainer, 2026-11-25): asked which name, how renaming is kept, and what several parameters from one node show, and offered "the node's face title as a section header, one header per source node, with the renamed label saved in the graph and the inner name left alone", they answered: "Go with your lean".
- Issue: none
- Assumed, for the maintainer to confirm: in a group inside a group the header is the innermost node's title; a label reset to a name already taken in the group gets a number ("seconds 2"); egui shows the header as a small "Title ›" line before the row.
- Done: 2026-11-25, branch `pr/fr5-promoted-labels`: a collapsed group's face lists its promoted rows under a header per source node, with the node's title; a promoted row's own menu has Rename Parameter… and Use the Port's Own Name. Mac, iPad and iPhone launched (a scripted copy and simulators); Windows built, not launched; egui by its test.

### FR3. An optional status line in the native shells, with what egui's status bar shows

- Reported: 2026-11-25
- Report: "The information in the original egui status line was timely, centralized, and includes more than I can easily see now. Analyze the original egui status and provide it as an option status line for the editor."
- Context: asked "Where is the status information that used to be at the bottom of the window?"; the Mac and iPad shells had none.
- Design: research note 28 (`docs/research/28-status-line.md`). The core gives the line as segments (`statusLine` in `ui.inspect` and the feed); View › Status Line (`view.statusLine`) shows or hides it, kept in preferences.
- Assumed, for the maintainer to confirm: on by default; an iPhone-width window keeps the first-priority segments only; the zoom stays where each shell has it; a selection count is new to the line; the running time moves with each core event, not a clock; Windows uses fixed width bands.
- Done: 2026-11-25, branch `pr/status-line`. Mac, iPad and iPhone launched (a scripted copy and simulators); Windows built, not launched; egui keeps its own bar, which the option hides.
- Issue: none

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
