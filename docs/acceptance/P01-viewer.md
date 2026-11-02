# P01: the first PlumCraft window, on the text engine

*2026-10-27. Checks that the interface carries node status and data in flight to a window, in both hosting modes. Not an editor yet: the graph is fixed, and only widget values change.*

Start it:

```sh
cd ~/workspace/plumcraft
cargo run -p plumcraft-view                # mode 1, engine compiled in
cargo run -p plumcraft-view -- --process   # mode 2, engine as a separate process
```

Build the engine binary first for mode 2 (`cargo build`), since the window spawns `target/debug/plumcraft-text-engine`.

## Checklist

| # | Try | Expect | Result |
|---|---|---|---|
| 1 | Open it (mode 1) | Top bar: "Text 0.1.0 · Rust · mode 1 · in-process", a green dot and "connected · 9 nodes". Seven cards in four columns, joined by wires. No states yet | |
| 2 | Press **Run** | `story` turns done, then `think` runs with a progress bar and percentage for 2 s, then `swap`, `count` and `shout` turn done. `broken` is red ("this node always fails") and `after` is grey, "skipped". Top bar: "finished with errors · ~2000 ms" | |
| 3 | Watch the right panel during the run | "shout.text" shows "waiting for the value…" until `shout` finishes, then the upper-case text, before the run ends | |
| 4 | Click the `count` card's title row (its name) | It gets a thick border; a new run starts; Watched shows `count.count` with the number 9 | |
| 5 | Edit `story`'s text, or `swap`'s find and with, then **Run** | Thumbnails and the watched text follow your edit || Works (maintainer, 2026-10-27) |
| 6 | Set `think` to 10 s, **Run**, then **Cancel** halfway | `think` ends "cancelled" within half a second; `swap`, `count`, `shout` show cancelled; the top bar says "cancelled" | |
| 7 | **Engine…** → "Rust, separate process (mode 2)" (was a Switch button before 2026-10-27) | The top bar shows "Rust · mode 2 · process NNNN"; the same runs behave the same | |
| 8 | In mode 2, set `think` to 10 s, **Run**, then **Kill engine** mid-run | The run fails with "engine: the engine stopped (signal 9); restarting". The dot goes red then green with a new process number. **Run** works again | |
| 9 | The Events panel | One line per status change with its `#seq`, the run's reply, and engine state changes | |
| 10 | Anything that looks wrong, slow, or confusing | Note it here | |
| 11 | `cargo run -p plumcraft-view -- --python` (added 2026-10-27) | Top bar: "Python · mode 2 · process NNNN". Runs look and time the same as in Rust; the event log is the same sequence || Covered (maintainer, 2026-10-27) |
| 12 | With the Python engine: cancel `think` mid-run, then **Kill engine** mid-run | As items 6 and 8: cancelled within half a second; then "the engine stopped (signal 9); restarting", a new process number, and **Run** works again || Covered (maintainer, 2026-10-27) |
| 13 | **Engine…** menu: switch between all three while idle | Each connects and the same graph runs; the top bar names Rust or Python and the mode || Covered (maintainer, 2026-10-27) |
| 14 | Any mode: **Run**, then **Run** again without changing anything (added 2026-10-27, the memo) | The second run finishes in about 0 ms. Every card except `broken` and `after` says "cached", in teal; `think` shows no progress bar. `broken` errors again (errors are never stored) || Works (maintainer, 2026-10-27) |
| 15 | Then change `swap`'s *with* and **Run** | `story` and `think` stay "cached" (no 2 s wait); `swap`, `count` and `shout` say "done" with the new text || Works (maintainer, 2026-10-27) |
| 16 | Click a different card's title row to watch it | The new run is instant (all cached) and Watched shows that card's value || Works (maintainer, 2026-10-27) |

## Results

- 2026-10-27, the maintainer, on first look: "Looks good." The checklist was not reported item by item.
- 2026-10-27, the maintainer: "11-13 covered". The Python engine passes in the window: mode label, cancel, kill and restart, and switching engines.
- 2026-10-27, the maintainer: "Can't edit the text fields to test that part." A click area over the whole card sat on top of the fields and took their clicks (and started a run). Now only the title row watches; recheck items 4, 5, 15 and 16.
- 2026-10-27, the maintainer, after the fix: "Works now." Field editing and the memo (items 5, 14–16) confirmed by hand.
- Follow-up asked for: externalize colours and theme materials at an appropriate time (`ROADMAP.md`).
