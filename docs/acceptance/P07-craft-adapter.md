# P07: the craft MCP adapter, VectorCraft first (M2)

*2026-10-27. `plumcraft-craft`: an artcraft app's MCP server as engine nodes, generic over a manifest (`crates/craft/packs/vectorcraft.toml`, `soundcraft.toml`). M2 is engine-only: the editor has no VectorCraft palette or SVG view until M3 (plan 10.6), where the hand-built trace-and-recolor graph is checked. These items run in a terminal.*

```sh
cd ~/workspace/plumcraft
cargo build
```

**Seen before the session (2026-10-27, the AI):** one run of the chain tests hung for over five minutes (six reruns took 2–3 s each). The test process died and left its `vectorcraft-cli` child running at full CPU with parent 1, which then had to be killed by hand. So the per-call timeout didn't end that hang, and closing stdin doesn't stop a busy craft process when the engine dies. Not yet explained; item 4 checks for it.

## Checklist

| # | Try | Expect | Result |
|---|---|---|---|
| 1 | `cargo test -p plumcraft-craft -- --nocapture 2>&1 \| grep -E "byte-identical\|test result"` | Every `test result` line is `ok` (41 tests); two lines say "SVG byte-identical to vectorcraft-cli run: 1265528 bytes" (mode 1 and mode 2) | AI, 2026-11-02: 94 tests pass, both lines (1265533 bytes on VectorCraft 0.20.0) |
| 2 | `cargo run -p plumcraft-craft --bin plumcraft-craft-engine -- --craft vectorcraft --smoke claude-debug/craft-mcp/corgi.jpg` | 142 lines starting `ok`, none `FAIL` (every curated node, effect and `vc.run`, with defaults, on the corgi) | AI, 2026-11-02: 156 of 156 nodes `ok`, none `FAIL`, exit 0, on the release build, after the smoke was limited to the pack's own nodes |
| 3 | `cargo test -p plumcraft-craft --test conformance soundcraft -- --nocapture` | `soundcraft_runs_from_its_manifest ... ok`: SoundCraft runs from its manifest alone (480 commands listed, a session saved and inspected at 44100 Hz), with no SoundCraft code in the crate | AI, 2026-11-02: passes, 483 commands on SoundCraft 0.16.0 |
| 4 | Afterwards: `pgrep -fl "vectorcraft-cli\|soundcraft-cli"` | Nothing: no craft processes are left | AI, 2026-11-02: only the Claude sessions' own craft MCP servers, as before |
| 5 | Anything wrong, slow, or confusing | Note it here | The test run takes minutes now: `panic_hunt` took 132 s |

## Results

- 2026-10-27, the maintainer, in a dialog: items 1–3 work (41 tests with both byte-identical lines, 142 smoke nodes ok, SoundCraft from its manifest). Item 4 showed `soundcraft-cli mcp` and `vectorcraft-cli mcp` running. The AI checked them: their parent is a Claude Code session (its configured craft MCP servers, started without the adapter's `--automation-read-root`), not the adapter or its tests, so item 4 is clean for the adapter. The earlier orphan from a hung test run (above) is still unexplained. Open: the hang.
- 2026-10-27, after the session: the orphan is fixed. Each craft process gets a watchdog in its own group that SIGKILLs the group once the engine is gone (`mcp.rs` `watchdog`); regression test `a_craft_dies_with_its_engine` (fails without the watchdog), and a `kill -9` of `plumcraft-craft-engine` mid-smoke left no craft process within 3 s. The hang itself is still open.
- 2026-11-02, the AI, at the maintainer's word ("Close out po7-9"), on `main` plus the `zip` 9 bump, VectorCraft 0.20.0 and SoundCraft 0.16.0: items 1–4 pass, as the table records. The first smoke run printed 10 `FAIL` lines and exited 1: the engine now carries the generic building blocks, which the smoke ran with no inputs. It now runs the pack's nodes only. Hunting the hang, the 18 chain tests ran 50 times (`claude-debug/craft-mcp/hang_loop.sh`): no run came near the 120 s limit, so **the hang was not reproduced and is still not explained**; the watchdog still bounds what it can leave behind. Two of the 50 failed differently: `a_cached_rerun_is_all_cached` got `done` where it wanted `cached`, both times with editors running beside it. Alone it passed 29 in a row at about 7 s each. A rerun losing its memo costs time, not correctness, and the cause isn't established. **Closed**, with that flake and the hang open in `docs/STATUS.md`.
