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
| 1 | `scripts/check` | ruff and format clean; every test passes | |
| 2 | `scripts/run --watch sentence.text`, then `scripts/nodes` | The demo's events, then "a corgi naps" and a summary with every node done; the catalogue | |
| 3 | `scripts/cli-run --watch loud.text`, then with `--http` | The same demo through PlumCraft's Rust CLI, which starts the engine from its manifest | |
| 4 | Open the folder in PyCharm | It offers the uv environment (or add `.venv/bin/python` as an existing interpreter); `src` is a source root and `tests` a test root; `.venv` is excluded | |
| 5 | The run configurations list | Serve (HTTP 7311), Run demo, Run demo (attached), Nodes, Tests, Conformance, Editor | |
| 6 | Run each of Nodes, Run demo, Tests and Conformance | Each does what its name says, with no setup | |
| 7 | Run Serve (HTTP 7311), then Run demo (attached) | The demo runs against the server | |
| 8 | Set a breakpoint in `shout`, then Debug Run demo | It stops there with `text` and `marks` in the variables pane; Resume finishes the run | |
| 9 | Debug Serve (HTTP 7311). In the editor: Engine › Attach…, `http://127.0.0.1:7311`, no token. Open `examples/demo.json` and run | The nodes appear in the palette and the breakpoint stops the run; the editor shows the node running; Resume finishes it | |
| 10 | In the editor, watch the SVG, audio, table and array outputs | Each shows in its viewer | |
| 11 | Run Editor | The editor opens with Atlas's demo, the engine started by the editor; Run finishes every node | |
| 11a | Edit `shout`, Engine › Reconnect, run | The change is there, the graph kept | |
| 11b | `scripts/install`, then Engine › Reload Engine List in another editor window | Atlas appears in the Engine menu, as a process and as HTTP; `scripts/uninstall` and reload take it away | |
| 11c | Engine › Add Engine › Array, then wire `ramp` into an array node | Both engines run one graph | |
| 12 | Follow "Adding a node" in the generated README | The steps work as written, including `UPDATE_GOLDEN=1 uv run pytest` | |
| 13 | Is anything missing, wrong or confusing for someone new to PlumCraft? | Note it here | |

## Results
