# PlumCraft engines in Python

`plumcraft_engine` is PlumCraft's engine SDK in Python: protocol 2's executor, host, the mode 2 socket and mode 3 HTTP fronts, and blobs in commonkit. `plumcraft_text` is the text engine written with it, kept byte-identical on the wire to the Rust `plumcraft-text` by the conformance suite (`cargo test -p plumcraft-text`).

## Install

```sh
pip install plumcraft-engine
plumcraft-engine new my-engine --name Mine   # a whole engine project, with tests and scripts
```

Python 3.11 or later. Running an engine needs only this package. The editor is Rust: `cargo install plumcraft-editor` ([PlumCraft](https://github.com/seantrue/plumcraft)).

## An engine in 20 lines

```python
import sys
from typing import Annotated, Literal

from plumcraft_engine import ConfigError, Cx, Engine, Range, WireType, node

Text = Annotated[str, WireType("TEXT", media="text", color="#c9a227", thumb=True)]


class Shout(Engine):
    id, name, version = "shout", "Shout", "0.1.0"
    category = "Text"

    @node("Loud")
    def loud(self, text: Text, marks: Annotated[int, Range(0, 5)] = 1, case: Literal["upper", "title"] = "upper") -> Text:
        """Shout the text."""
        if not text:
            raise ConfigError("nothing to shout")
        return (text.upper() if case == "upper" else text.title()) + "!" * marks

    @node("Wait")
    def wait(self, text: Text, seconds: float = 1.0, *, cx: Cx) -> Text:
        cx.progress(0.5, "waiting")
        return text


if __name__ == "__main__":
    sys.exit(Shout.serve())
```

That is the whole engine. The class is compiled when it is defined: each `@node` method becomes a NodeDef (`shout.loud`: a `TEXT` socket, an `INT` widget with `min`/`max`, an `ENUM` widget with choices, a `TEXT` output), and each mistake raises `DeclarationError` at import, not mid-run. `Shout()` builds the engine; `Shout` itself is the factory `Host` and the bindings take.

## Ports from annotations

| Python | Port |
|---|---|
| `str`, `int`, `float`, `bool` | `STRING`, `INT`, `FLOAT`, `BOOL`: a widget that can also be wired (no `socket` field, the proto's default) |
| `Literal["a", "b"]` | `ENUM` with `choices: ["a", "b"]` |
| `Annotated[T, WireType("ID", media=...)]` alias | a socket of type `ID` (`socket: "socket"`); declare the alias once, at module level |
| `String`, `Int`, `Float`, `Bool` (from the SDK) | the scalar types as explicit aliases, for `wires` and annotations |
| `Text`, `Table`, `Image`, `Audio`, `Array`, `Svg`, `Json` (from the SDK) | the core types, exactly as every engine declares them (`docs/interface.md`): a node takes and returns a `str`, a columnar table `dict`, an encoded-picture `Blob` (make one with `image(data)`), an `AudioValue(samples, sample_rate)`, an ndarray or `ArrayValue`, an SVG `Blob` (`svg(text)`), and any JSON value |
| `= default` | `default` (an `int` default on a `float` port becomes a float) |
| `X \| None = None` | `optional: true` |
| `Annotated[float, Range(0, 1, 0.1)]` | `min`, `max`, `step` |
| `Annotated[str, File("open", (("Text", ("txt",)),))]` | `path: {mode, filters}`; `open`, `save` or `dir` |
| `Annotated[T, Port(label=, tooltip=, advanced=, socket=, name=, decimals=)]` | those fields |
| a parameter annotated with a `@dataclass` | one port per field (with the field's annotation and default); the method receives the dataclass |
| a parameter named `cx` (or annotated `Cx`) | not a port: the node's `Cx`, for `cx.progress(f, stage)` and `cx.cancelled()` |
| `with_` | the port `with` (one trailing underscore is dropped, for keywords) |

Outputs come from the return annotation:

| Return | Outputs |
|---|---|
| `-> Text` (any port type) | one output, named after the wire type's id in lower case (`text`), or `@node(output="name")` |
| `-> Pair` (a `NamedTuple`) | one output per field, typed by the field; return the tuple. `output=("a", "b")` renames them |
| `-> None` | none (a sink) |

## `@node` options

`@node` alone, or `@node("Display name", ...)` with any of: `id` (default `<engine id>.<method>`), `category` (a string or a path; default the engine's `category`), `description` (default the docstring), `output`, `family` and `icon` (default the engine's), `version` (part of the memo key; bump it when results change), `memo` (a scope such as `"none"`, or a whole `memo` object), `flags`, `x` (passed through).

Engine class attributes: `id`, `name`, `version`, `salt` (default `py:<id>+<version>`), `dependencies` (the packages results depend on, such as `("opencv-python-headless",)`: their versions join the memo salt, so an upgrade never reuses an old result), `category`, `family`, `icon`, and `wires`: the wire types in catalogue order. Without `wires`, the catalogue lists the types in the order the nodes first use them; with it, a node using a type not listed is a `DeclarationError`. Subclasses inherit nodes and may override them.

## Values and errors

Before the method runs, every input is checked against its port:

- a `str` port (or a wire type over `str`) receives any value as text: strings as they are, other values as compact JSON;
- an `int`, `float`, `bool` or `Literal` port that gets the wrong type, or a number outside its `Range`, fails the node with a `config` error naming the port (`marks: 9 is outside 0..5`);
- `Range(..., invalid="clamp")` clamps instead, and replaces a wrong type with the default; `Range(..., invalid="pass")` makes the range a UI hint only and hands the value over as it is.

Raise `ConfigError("...")` for a setting the user can fix, `OpError("...")` for a failure on this input, or `NodeError.cancelled()` when `cx.cancelled()`. Any other exception fails the node as `op` with its type and message, and the engine carries on.

## Serving

`MyEngine.serve()` reads the command line: `--connect unix:<path>` (or `PLUMCRAFT_ENGINE_SOCKET` and `PLUMCRAFT_ENGINE_TOKEN`) serves mode 2, `--listen host:port [--port-file f] [--lifeline]` serves mode 3. In a test, `Host(MyEngine)` runs it in process.

**Pictures on nodes:** a picture file's value shows a thumbnail when it carries one. Make it with `image(data, pixels=array)`, or override `Engine.picture_thumb(blob)` to give Load Files' values one, returning (h, w, 4) uint8; the SDK itself decodes no pictures.

## From the command line

`plumcraft-engine` drives an engine with no Rust build:

```sh
uv run plumcraft-engine nodes                                  # the catalogue
uv run plumcraft-engine run                                    # the [tool.plumcraft] demo, events as they come
uv run plumcraft-engine run my.json --watch shout.text         # a workflow, with a value watched
uv run plumcraft-engine run my.json --cancel-after 0.5 --raw   # cancel mid-run; every event as JSON
uv run plumcraft-engine run my.json --attach http://127.0.0.1:7311   # an engine serving --listen
```

- **The engine:** `--engine module:Class`, else `[tool.plumcraft] engine` in the nearest `pyproject.toml` (this one names `plumcraft_text:TextEngine`, with `demo = "../examples/data.json"`). It runs in process, as the editor's in-process mode does; `--attach URL` talks to one already serving HTTP, with `PLUMCRAFT_ENGINE_TOKEN` when set.
- **The workflow:** path inputs relative to the workflow's folder are made absolute, as `plumcraft run` does.
- **The exit code:** 0 when every node is `done` or `cached`, 1 when one isn't, 2 when the engine or workflow can't be had. A script can use `run` as a smoke test.

## A new engine

```sh
uv run --project ~/workspace/plumcraft/python plumcraft-engine new ~/workspace/weather-engine --name Weather --types text
```

This writes a standalone uv project that depends on this SDK, which `scripts/editor` opens in PlumCraft's editor, the editor starting the engine from its manifest: the engine with example nodes for each type, its tests (including a golden catalogue and conformance), a demo workflow, scripts for testing from the command line, PyCharm run configurations, a README for the developer, and the engine manifest. It refuses a directory with something in it and ids PlumCraft already uses, and a failure leaves nothing behind.

- **`--types`:** `text`, `table`, `array`, `svg`, `image` (adds pillow), `audio`, `json`, `list` (brings `text`), `any` (the building blocks), comma-separated. Scalars are always on. A conversion is generated when both its types are chosen, always with its inverse: `table`↔`text` (CSV), `table`↔`array`, `svg`↔`text`, `image`↔`array`, `audio`↔`array`, `json`↔`text`.
- **The other options:** `--id`, `--name`, `--package`, `--sdk pypi|path|git[@REV]`, `--python`, `--no-pycharm`, `--git`.

`uv run plumcraft-engine check` runs PlumCraft's conformance cases against an engine, in process and over HTTP (`--mode` for one): the session, core types declared exactly, every node on its defaults, the demo, cancel, answers off the run path, and hostile widget values. It needs only the catalogue, plus `[tool.plumcraft] demo` and `[tool.plumcraft.conformance] slow` when set; in a test, `plumcraft_engine.conformance.assert_conformant(MyEngine, demo=..., slow=...)` fails naming every failed case.

## Alternatives considered

1. **A parameters dataclass per node as the only form** (the research software's `parameters = AnnealParameters`). It reads well with ten settings and badly with one, and it splits a node's inputs between the signature (sockets) and the dataclass (widgets). It is supported as a parameter type for nodes with many settings.
2. **`Port(...)` as the default** (`find: str = Port("", label="Find")`, as typer and pydantic do). It hides the real default from type checkers and from readers. `Annotated` keeps the default a default.
3. **Signatures plus `Annotated`** (chosen): the method is an ordinary typed function a type checker understands, the port list is the parameter list, and metadata sits beside the type it qualifies.

## The low-level layer

The decorators compile to the layer underneath, which stays and is the escape hatch: a `Node` subclass with a `definition` dict (a NodeDef in protocol 2's JSON) and `run(inputs, cx) -> dict`, or `simple(definition, fn)`, added with `engine.with_node(...)` to a declared engine or to `Engine(info)`. Use it for what a signature can't say yet: variadic ports, `options`, `list` ports, `active_when`, `disabled_choices`, or ports whose shape depends on configuration.
