# P06: the Engine menu

*2026-10-27. Switch engine and hosting while the editor runs: the Engine menu (Text and Audio choices, Reconnect, Attach…) and the `engine.switch`, `engine.reconnect`, `engine.attach` and `engine.list` commands.*

```sh
cd ~/workspace/plumcraft
cargo build                          # builds the engine binaries the menu starts
cargo run -p plumcraft-editor
```

## Checklist

| # | Try | Expect | Result |
|---|---|---|---|
| 1 | Open the **Engine** menu | Last in the menu bar, after Group. "Text" has five choices (Rust in-process, Rust process, Python process, Rust HTTP, Python HTTP); "Audio" has three; the current one is selected; Reconnect and Attach… below | |
| 2 | Run the demo, then pick each Text choice in turn and run after each | The graph stays; old results clear on the switch; the run works every time; the status bar and window title name the engine and hosting (e.g. "Text · Python · process (mode 2)") | |
| 3 | **Reconnect** | The same engine restarts and the graph runs | |
| 4 | Set the demo's Slow node (`think`) to 20 seconds, run, then switch engines mid-run | The run is cancelled, and the new engine connects | |
| 5 | Switch to an Audio choice | Nothing is deleted; the status bar says how many problems the graph has with this engine (unknown text nodes). Switch back to Text and run: it works | |
| 6 | Attach: in another terminal `PLUMCRAFT_ENGINE_TOKEN=secret cargo run -p plumcraft-text --bin plumcraft-text-engine -- --listen 127.0.0.1:7320`, then **Attach…** with `http://127.0.0.1:7320`, first a wrong token, then `secret` | The wrong token is refused with a reason; the right one connects, and the graph runs | |
| 7 | Afterwards, `pgrep -fl plumcraft` with the editor closed | No engine processes are left, apart from the one you started by hand for item 6 (stop it with Ctrl-C) | |
| 8 | Anything wrong, slow, or confusing (switching blocks the window briefly, as at startup) | Note it here | |

## Results

- 2026-10-27, the maintainer, all items in a dialog: 1–8 work (the menu, every Text choice with a run after each, Reconnect, cancel on a mid-run switch, text to audio and back keeping the graph, Attach with a wrong then the right token, no engine processes left, nothing to note). P06 accepted.
