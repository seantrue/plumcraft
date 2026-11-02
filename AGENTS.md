# AGENTS.md: guide for AI agents and contributors

**The goal:** a general-purpose, native node-flow editor whose engines are interchangeable. Any engine (compiled in, a separate process, HTTP, or pipekit/AMQP) speaks one interface, `docs/interface.md`, and the editor never knows which engine it has. The research software supporting www.virtimp.com is the first engine pack and the reference: **PlumCraft must never support less than the research software uses** (the floor), including node status tracking and data in flight.

Read this file, then `docs/STATUS.md`, then `docs/plan.md`.

## Golden rules

Adapted from the research software's `AGENTS.md`.

1. **Never crash.**
   - Non-test code never panics: no `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!` or `unimplemented!`.
   - Every crate starts with `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`; `clippy.toml` allows them in tests only.
   - Every message from the other side is hostile input (spec section 18). Parsing never panics; an unknown kind, locator or state is skipped, not fatal. Every new parser joins the hostile-input sweep (`crates/proto/tests/hostile.rs`).
2. **No `unsafe`**, except a future shared-memory reader crate, for `Mmap::map`, with a `SAFETY:` comment on every block.
3. **No engine in core.** No node id, port name or wire type is hard-coded beyond the scalars `FLOAT INT BOOL STRING ENUM` and the core types in `plumcraft_proto::catalogue::core_types`, today `ARRAY` (maintainer, 2026-10-29: "Make arrays first class"; an array means the same thing in every engine, so values cross engine borders unchanged) `SVG` (maintainer, 2026-10-30: "Make SVG first class"), and `TEXT`, `TABLE`, `IMAGE`, `AUDIO` and `JSON` ("yes core data types", "Core audio, please", "JSON yes"; their bytes are in `docs/interface.md`). Pack data stays opaque (`x.<pack>`). Protocol 1 (the research software's own protocol) is mapped on the research software's side, never here.
4. **Everything is a command** in the editor, as in the crafts: every user action has an id, label, `enabled` and `run`, and the UI, CLI, control channel and MCP dispatch by id.
5. **The interface is the contract.** A change to a message changes `docs/interface.md` in the same commit. Additive changes are capabilities, never minor versions.
6. **Fix bugs at source.** commonkit, pipekit, the research software and the crafts are the maintainer's own repos: fix them there or propose the fix.
7. **Tests are the gate.** Unit tests, the hostile-input sweep, shared test vectors (`crates/proto/fixtures/`), and, as engines arrive, the conformance suite every engine must pass.

## Before you finish a task

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
(cd python && uv run pytest && uv run ruff check .)
```

A change to a message type regenerates the schema: `cargo run -p plumcraft-proto --example export_schema --features schema` (a test fails while `schema/protocol2.json` is stale). Update `docs/STATUS.md`. Update `AI.md` whenever you write a commit message (the `ai-md` skill). Commit only when asked.

## Scratch scripts and data

Probes, scratch scripts and test data go in `claude-debug/`. Only `.md` notes there are tracked. Don't delete what's there, and avoid name collisions. Temporary files a test writes go under its process's scratch root (`plumcraft_scratch::root()` / `dir()` in Rust, `scratch.mkdtemp()` in `python/tests`), never straight into `$TMPDIR`: the next run sweeps the roots of exited processes. `claude-debug/make_handle_vectors.py` regenerates `crates/proto/fixtures/handles.json` with the research software's engine venv.

Reports are Markdown with blank lines around headers and tables, converted to PDF with pandoc (`--pdf-engine=xelatex`); keep the `.md` source.
