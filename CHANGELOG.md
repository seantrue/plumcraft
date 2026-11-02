# Changelog

## Installers 0.1.0, 2026-11-02

[Installers](https://github.com/seantrue/plumcraft/releases/tag/app-v0.1.0) for macOS on Apple silicon and x86-64 Linux, with every engine, audio included.

## plumcraft-engine 0.1.1, 2026-11-02

Generated engines now say how to get the Rust tools, `cargo install plumcraft plumcraft-editor`. 0.1.0's message named a package that was renamed.

## 0.1.0, 2026-11-02

The first public version. The Python SDK, `plumcraft-engine`, is on [PyPI](https://pypi.org/project/plumcraft-engine/). The editor is [`plumcraft-editor`](https://crates.io/crates/plumcraft-editor) and the command line [`plumcraft`](https://crates.io/crates/plumcraft) on crates.io. The audio engine builds from a checkout, and `plumcraft run --python` needs one too.

- A native node-flow editor with interchangeable engines, speaking protocol 2 in-process, over a Unix socket or over HTTP.
- Engines for text (Rust and Python), audio and arrays, plus craft apps through their MCP servers.
- Several engines in one graph, with values shared by content handle.
- A Python engine SDK, and `plumcraft-engine new` to generate an engine project.
- `plumcraft-engine new` works from an installed package: `--sdk pypi`, the default outside a checkout.
- `zip` 9, for three advisories against 3.0. A bundle entry named from a root or a drive is refused.

See `docs/guide.md` for what's built and `docs/STATUS.md` for current work.
