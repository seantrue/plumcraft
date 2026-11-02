# Contributing

Thanks for your interest. PlumCraft is pre-release, and its interface can still change.

## Before you start

Read `AGENTS.md` for the rules every change follows, then `docs/STATUS.md` for where things stand. For anything large, open an issue first.

The rules that come up most often:

- Non-test code never panics, and there is no `unsafe`.
- Every message from an engine is treated as hostile. New parsers join the sweep in `crates/proto/tests/hostile.rs`.
- No engine-specific names in core.
- Every user action is a command.
- A change to a message changes `docs/interface.md` in the same commit. Additive changes are capabilities, not minor versions.

## Checks

These must pass before a pull request:

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
(cd python && uv run pytest && uv run ruff check .)
```

Tests that need a craft app skip when it isn't installed. If you change a message type, regenerate the schema:

```sh
cargo run -p plumcraft-proto --example export_schema --features schema
```

## Licence

PlumCraft is licensed under either the MIT licence or the Apache License 2.0, at your choice. Unless you say otherwise, any contribution you submit for inclusion is licensed the same way, with no additional terms.
