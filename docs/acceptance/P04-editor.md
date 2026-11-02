# P04: the editor (E1)

*2026-10-27. PlumCraft's editor, lifted from the research software's and generalised: edit the graph on a node canvas, run it on any engine, watch values; every action a command, drivable over the control channel. Not yet: files and undo, groups, an Engine menu (choose with a flag).*

```sh
cd ~/workspace/plumcraft
cargo run -p plumcraft-editor                 # Rust engine in-process; or --process, --python, --http [--python]
```

## Checklist

| # | Try | Expect | Result |
|---|---|---|---|
| 1 | Open it | The demo graph as a node canvas: seven nodes in columns, wires coloured by type, widgets on the nodes; the status bar names the engine and mode | |
| 2 | **Run** (Cmd+Enter) | Each node shows its state and time; `think` shows progress for 2 s; Fail is red with its error; Lower case "skipped" | |
| 3 | Run again | Every node but Fail and Lower case "cached", instantly | |
| 4 | Right-click the canvas | The palette, with search; pick "Word count": a node appears where you clicked | |
| 5 | Drag from `swap`'s text output to the new node's input | A wire; drag from Word count's `count` (INT) onto a TEXT input: refused, with the reason | |
| 6 | Edit `swap`'s *with*, run | Only `swap` and what's downstream recompute; stale nodes say why on hover before the run | |
| 7 | Click a node's title | The Watched panel shows its value (full text, a number) | |
| 8 | Select a node, delete it (menu or key); drag nodes around | It goes with its wires; positions follow | |
| 9 | **Run → Auto-Run**, then edit a value | It runs by itself shortly after each edit | |
| 10 | `--python` and `--http --python` | The same graph runs the same on the Python engine, local and over HTTP | |
| 11 | Optional, the control channel: `cargo run -p plumcraft-editor -- --control 0 --control-token-file /tmp/pc.token`, then `python3 claude-debug/editor/control_probe.py` (see `docs/control-protocol.md`) | The probe adds, wires, runs and watches nodes in your window and writes a screenshot | |
| 13 | (E2) Edit something, **Edit → Undo** (Cmd+Z) and **Redo** (Cmd+Shift+Z) | Each step undoes and redoes; the menu names the step ("Undo Set Value") | |
| 14 | (E2) Select nodes, Cmd+C, click elsewhere, Cmd+V | Copies with fresh ids, wires among them kept | |
| 15 | (E2) **File → Save As…**, then quit and `cargo run -p plumcraft-editor -- <that file>` | The title shows the file name, with • while unsaved; the graph comes back with positions and values | |
| 16 | (E2) Edit, then **File → New** or **Open** | "Unsaved changes" asks first; Cancel keeps the graph | |
| 17 | (E2) `cargo run -p plumcraft-editor -- examples/data.json` | `readme` loads `../README.md` relative to the workflow's folder (now so in the CLI and viewer too) | |
| 18 | (E3) Select think, swap and shout; **Group → Group** (Cmd+G) | They become a group; its header chevron collapses it to one node | |
| 19 | (E3) On the group, promote `swap`'s *find* (node menu or group menu) and an output | The collapsed group shows the widget and an output pin; wire from it, edit the widget, run | |
| 20 | (E3) Enter the group (pencil), edit inside, leave by the breadcrumb; then Cmd+Shift+G | Ungroups; Cmd+Z brings the group back | |
| 21 | (E3) Save, reopen | The group comes back collapsed, with its promotions; a run gives the same results as before grouping | |
| 22 | (E3) **File → Insert as Group…** another workflow | It arrives as one collapsed group | |
| 12 | Anything that looks wrong, slow, or confusing | Note it here | |

## Results

- 2026-10-27, the maintainer, after E1.3: "plumcraft-editor works as described."
- 2026-10-27, the maintainer: "P04 is good". Items not reported one by one.
- 2026-10-27, the maintainer, E2/E3 items in a dialog: 13–21 work. Comments: "Dragging the copied nodes should move them all"; "closing app with unsaved graph should confirm"; item 22 fails: "Insert as group, arrives as a uncollapsed group which cannot be moved. If manually collapsed it can be moved, but uncollapses to the insertion position".
- 2026-10-27, fixes for the three findings above (uncommitted): drags keep the node under the pointer for the whole drag, so pasted and duplicated selections move together, collapsed groups included; closing over unsaved changes prompts (Save / Discard / Cancel; File › Quit, Cmd+Q; macOS default app menu turned off so Cmd+Q can be cancelled); insert-as-group arrives collapsed and selected, and a group move moves its members, so a group expands where it was moved (Cmd+G groups too). To re-check: items 14, 16 (and quitting), 22.
- 2026-10-27, the maintainer, re-check after the fixes: 14 (paste and drag), 16 (unsaved: New/Open), 16b (unsaved: quit) and 22 (insert as group) all work. P04 items 1–22 are accepted.
