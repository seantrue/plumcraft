# P09: the engine generator in PyCharm

*2026-10-30. A generated engine as a human developer meets it: `plumcraft-engine new`, the scripts, and PyCharm's run configurations and debugger. The generator's own tests cover what doesn't need PyCharm; this checklist covers what does.*

```sh
cd ~/workspace/plumcraft
cargo build --release -p plumcraft-cli -p plumcraft-editor
uv run --project python plumcraft-engine new ~/workspace/atlas-engine --name Atlas --types text,table,array,svg,image,audio,list,any
cd ~/workspace/atlas-engine
uv sync
```

## Checklist

| # | Try | Expect | Result |
|---|---|---|---|
| 1 | `scripts/check` | ruff and format clean; every test passes | AI, 2026-11-02: passes, 29 tests |
| 2 | `scripts/run --watch sentence.text`, then `scripts/nodes` | The demo's events, then "a corgi naps" and a summary with every node done; the catalogue | AI: passes, 30 nodes done |
| 3 | `scripts/cli-run --watch loud.text`, then with `--http` | The same demo through PlumCraft's Rust CLI, which starts the engine from its manifest | AI: passes, process and `--http` |
| 4 | Open the folder in PyCharm | It offers the uv environment (or add `.venv/bin/python` as an existing interpreter); `src` is a source root and `tests` a test root; `.venv` is excluded | Not run: needs PyCharm. The generated `.iml` has the source root, the test root and `.venv` excluded |
| 5 | The run configurations list | Serve (HTTP 7311), Run demo, Run demo (attached), Nodes, Tests, Conformance, Editor | AI: all seven are in `.run/`. Not seen in PyCharm |
| 6 | Run each of Nodes, Run demo, Tests and Conformance | Each does what its name says, with no setup | AI: each configuration's command run from a shell, and each works |
| 7 | Run Serve (HTTP 7311), then Run demo (attached) | The demo runs against the server | AI: passes the same way; without the token `/v1/health` is 401 |
| 8 | Set a breakpoint in `shout`, then Debug Run demo | It stops there with `text` and `marks` in the variables pane; Resume finishes the run | Not run: needs PyCharm. `cli run` starts the engine as a child process, so this depends on PyCharm following it; `pdb` alone never reached `shout` |
| 9 | Debug Serve (HTTP 7311). In the editor: Engine › Attach…, `http://127.0.0.1:7311`, the token in `.engine-token`. Open `examples/demo.json` and run | The nodes appear in the palette and the breakpoint stops the run; the editor shows the node running; Resume finishes it | AI: attach is refused without the token and works with it; the demo runs (30 nodes). The debugger part not run |
| 10 | In the editor, watch the SVG, audio, table and array outputs | Each shows in its viewer | AI: SVG, audio, table and array each open their viewer |
| 11 | Run Editor | The editor opens with Atlas's demo, the engine started by the editor; Run finishes every node | AI: 48 nodes, 30 done, started from the manifest |
| 11a | Edit `shout`, Engine › Reconnect, run | The change is there, the graph kept | AI: after the edit and Reconnect the panel shows `A CORGI NAPS???`, graph kept |
| 11b | `scripts/install`, then Engine › Reload Engine List in another editor window | Atlas appears in the Engine menu, as a process and as HTTP; `scripts/uninstall` and reload take it away | AI: Atlas in the engine list after install and reload, gone after uninstall. Not checked as process and HTTP separately |
| 11c | Engine › Add Engine › Array, then wire `ramp` into an array node | Both engines run one graph | AI: Atlas + Array, 102 nodes; `ramp` into `array.clamp` runs, through the control channel |
| 12 | Follow "Adding a node" in the generated README | The steps work as written, including `UPDATE_GOLDEN=1 uv run pytest` | AI: passes as written |
| 13 | Is anything missing, wrong or confusing for someone new to PlumCraft? | Note it here | AI: this checklist's item 9 still said "no token", which HTTP engines stopped allowing; corrected. The generated project needs a PlumCraft checkout until the SDK is on PyPI |

## Results

- 2026-11-02, the AI, at the maintainer's word ("Close out po7-9"), on `main` plus the `zip` 9 bump, release build: every item a shell or the control channel can reach (`claude-debug/vary/p09_probe.py`, `p09_probe2.py`, `p09_probe3.py`), and the table says which those are. **Closed, with items 4 and 8 and the debugger part of item 9 not run:** they need PyCharm, and nobody has tried them.
