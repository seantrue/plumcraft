# P02: the local data plane (D1)

*2026-10-27. Checks that values too big to send inline cross as blobs in commonkit, named by their handles, that tables and arrays have their own views, and that a file source is keyed by its content. (The example loads `../README.md`, relative to its own folder, since E2: run it from anywhere.)*

```sh
cd ~/workspace/plumcraft
cargo build
cargo run -p plumcraft-view -- examples/data.json --python
```

The same checks hold with `--process` (Rust engine) and with neither (mode 1).

## Checklist

| # | Try | Expect | Result |
|---|---|---|---|
| 1 | Open it and press **Run** | Six cards, all "done". `shout` and `many` show a short clipped excerpt, not the whole text | |
| 2 | Watch `shout` (click its title) | Watched: "text, 99000 characters", a line "99000 bytes from the store · md5:raw:5840…", and the whole text below (scroll to the end) | |
| 3 | Watch `abc` | A table: letter and count for a–z as found in the README, each with a bar | |
| 4 | Watch `codes` | "array <u4 (33,)", "inline · md5:nd1:…", and 33 bars (spaces lowest) | |
| 5 | Change `story`'s text, **Run**, watch `codes` again | The bars and handle change with the text | |
| 6 | Run twice; then change `readme`'s path to another file with the same contents (`cp README.md /tmp/r.md`, path `/tmp/r.md`) and **Run** | `readme` stays "cached": a file is keyed by its bytes, not its name. Edit `/tmp/r.md` and **Run**: now it recomputes | |
| 7 | Watch `shout`, then delete the store's copy: `rm $COMMONKIT_STORE_ROOT/plumraw/58/584059b192f1818391a56f8eefcc1f89.c.npy`, and **Run** | The event log says the blob was gone and reruns once; the text comes back | |
| 8 | **Engine…** → each engine in turn on the same graph | The same handles in the Watched line whichever engine runs it | |
| 9 | Anything that looks wrong, slow, or confusing | Note it here | |

## Results

- 2026-10-27, the maintainer, after loading a different text file in the viewer (Python engine, watching `abc`): "All good." Items not reported one by one.
