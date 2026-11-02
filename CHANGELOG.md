# Changelog

## 0.1.0, 2026-11-02

The first public version. The Python SDK, `plumcraft-engine`, is on [PyPI](https://pypi.org/project/plumcraft-engine/). The command line is `plumcraft` on [crates.io](https://crates.io/crates/plumcraft), with 15 of its library crates. The editor builds from source, and `plumcraft run --python` needs a checkout.

- A native node-flow editor with interchangeable engines, speaking protocol 2 in-process, over a Unix socket or over HTTP.
- Engines for text (Rust and Python), audio and arrays, plus craft apps through their MCP servers.
- Several engines in one graph, with values shared by content handle.
- A Python engine SDK, and `plumcraft-engine new` to generate an engine project.
- `plumcraft-engine new` works from an installed package: `--sdk pypi`, the default outside a checkout.
- `zip` 9, for three advisories against 3.0. A bundle entry named from a root or a drive is refused.

See `docs/guide.md` for what's built and `docs/STATUS.md` for current work.
