# P03: remote over HTTP (D2)

*2026-10-27. Checks that an engine reached over HTTP behaves like a local one, with sources uploaded by handle and results downloaded lazily, verified, into the UI's own CAS. Locally, `--http` talks to the engine as a UI on another machine would. Run from the repository root.*

```sh
cd ~/workspace/plumcraft
cargo build
cargo run -p plumcraft-view -- examples/data.json --http --python
```

## Checklist

| # | Try | Expect | Result |
|---|---|---|---|
| 1 | Open it and press **Run** | Top bar: "Python · mode 3 · HTTP, remote". Six cards turn "done", as in P02 | |
| 2 | Watch `shout` | Watched: "99000 bytes downloaded by URL, verified, kept in the UI's CAS", then the whole text | |
| 3 | Watch `abc`, then `codes` | The table and the bars, as in P02 | |
| 4 | Change `readme`'s path to another text file and **Run** | The table follows the new file (it was uploaded); **Run** again: `readme` and `abc` "cached" | |
| 5 | **Engine…** → "Rust over HTTP, remote (mode 3)" | The same results; the same handles in the Watched line | |
| 6 | Quit the window (or `kill -9` it), then `pgrep -fl plumcraft` | No engine left running | |
| 7 | Optional, a real remote: on a remote server, `cd ~/workspace/plumcraft/python && PLUMCRAFT_ENGINE_TOKEN=<t> uv run python -m plumcraft_text --listen 127.0.0.1:7311`; on the development machine, `ssh -N -L 17311:127.0.0.1:7311 engine.example` and `PLUMCRAFT_ENGINE_TOKEN=<t> cargo run -p plumcraft -- run examples/data.json --attach http://127.0.0.1:17311 --watch shout.text` | "(attached …, remote)"; the run finishes; a second run is all `cached` in well under a second. (a remote server has a copy of `python/` from this session; re-sync with `rsync -a --exclude .venv python/ engine.example:workspace/plumcraft/python/`) | |
| 8 | Anything that looks wrong, slow, or confusing | Note it here | |

## Results
- 2026-10-27, the maintainer: "Accept P03." Items not reported one by one.
