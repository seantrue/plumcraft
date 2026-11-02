# 17. An engine generator: a Python engine project for a human developer

The maintainer, 2026-10-30:

> "Start a task that will plan a engine generator. Inputs would be which data types would be used, and output is a python engine example in a new directory. It should have everything necessary to load and run it under plumcraft. This is intended for separate development, primarily by a human developer. Include scripts to run the engine in command line mode for testing. These engines will be largely developed in Pycharm, so scaffold them appropriately."

This note is a plan. Nothing here is built yet.

## 1. What exists today

### 1.1 The Python SDK

- **The declarative layer** (`python/src/plumcraft_engine/declare.py`, `python/README.md`):
  - An engine is an `Engine` subclass. Each `@node` method becomes a NodeDef, built from its signature when the class is defined.
  - Mistakes raise `DeclarationError` at import.
  - `MyEngine.serve()` is the whole entry point: `--connect unix:<path>` (or `PLUMCRAFT_ENGINE_SOCKET` and `PLUMCRAFT_ENGINE_TOKEN`) serves mode 2; `--listen host:port [--port-file f] [--lifeline]` serves mode 3 (`serve.py`, `http.py`).
  - An empty token is allowed on loopback only.
- **Wire types the SDK exports:**
  - The scalar aliases `String`, `Int`, `Float` and `Bool`.
  - `Literal[...]`, which becomes `ENUM`.
  - The core types `Array` (an ndarray, or `ArrayValue` for a colour space and channel names) and `Svg` (a `Blob` made with `svg(text)`).
  - `Blob`, for any encoded bytes.
  - `List`, for sweeps (spec 7.6).
  - `ANY`, but only through `building_blocks = True` (Vary, sequences, Select, Find, Load and Write Files).
- **Types it does not export:**
  - **`TEXT` and `TABLE` are declared by each engine**, not by the SDK. `plumcraft_text` declares `TEXT` (`#c9a227`, media `text`) and `TABLE` (`#4f86c6`, media `table`), and the Rust audio engine declares the same `TABLE`. The coordinator lets a non-core type cross from one engine to another only when both declare it with the same media (`crates/coordinator/src/merge.rs:130`). So a generated engine must copy these declarations exactly.
  - **There is no `AUDIO` and no `IMAGE` in the Python SDK.** `AUDIO` (`#2fa39a`, media `audio`, `Form::Audio`) exists only in Rust (`crates/audio`). `IMAGE` (`#2563eb`, media `image`) is the craft adapter's (`crates/craft/src/lib.rs:190`).
- **List ports** (`list: true`), variadic ports and `options` need the low-level layer (a `Node` subclass with a `definition` dict). `Port(...)` has no `list` field.
- **An `Svg` input is untested.** `python/tests/test_svg.py` covers outputs only.
- **In-process hosting needs no socket.** `plumcraft_engine.host.Host(MyEngine)` has `run`, `poll`, `get`, `cancel` and `schema`, and `python/tests/test_engine.py` drives it that way. A Python-only runner can be built on it.

### 1.2 Where the SDK lives

- **The SDK is not on PyPI.** `plumcraft-engine`, `plumcraft_engine` and `plumcraft` all return 404 from `pypi.org/pypi/<name>/json` (checked 2026-10-30).
- Its two runtime dependencies are on PyPI: `numpy>=1.26` and `pyuncommonkit>=0.1`.
- The repository `github.com/seantrue/plumcraft` is **private**.
- The SDK's wheel also bundles `plumcraft_text` (`[tool.hatch.build.targets.wheel] packages`).

### 1.3 How PlumCraft reaches an engine

- **Editor** (`apps/plumcraft-editor/src/main.rs`):
  - `EngineKind` is a closed enum: `Text`, `Audio`, `Array`, and `Craft(id)` for the built-in craft packs. `Mode` is `inproc`, `process`, `python`, `http` and `http-python`.
  - `--python` is valid for the text engine only. It runs `plumcraft_text::python_command()`, which is `uv run --quiet --project <checkout>/python python -m plumcraft_text`, with `$PLUMCRAFT_UV` overriding `uv`.
  - `Engine › Attach…` (`engine.attach`, `AttachForm {url, token}`) attaches to **any** running HTTP engine and keeps the graph.
  - `engine.reconnect` stops the engine and starts a fresh process, keeping the graph.
- **CLI** (`apps/plumcraft-cli`):
  - `plumcraft run [WORKFLOW] [--process|--python|--http|--attach URL] [--watch NODE.PORT] [--cancel-after S] [--raw]` and `plumcraft nodes [ENGINE]`.
  - Everything except `--attach` is the text engine. `--attach` reads `PLUMCRAFT_ENGINE_TOKEN` and works with any HTTP engine.
- **Hosts** (`crates/host`):
  - `process::Manifest {name, command, env, hint, store}` is an in-code struct.
  - `ProcessHost::start` (mode 2) and `HttpHost::spawn` (mode 3: the same command plus `--lifeline --listen 127.0.0.1:0 --port-file f`) both take any command, so nothing in the host is text-specific.
- **The engine manifest file is specified but not built.** Spec 16.6 (`docs/interface.md`) defines a TOML manifest (`id, name, mode, command, cwd, env, restart, max_restarts, url, token_file, …`) found in the app bundle, in `~/.config/plumcraft/engines/*.toml` or through `--engine-manifest`. Nothing reads it. `plumcraft_craft::manifest` is a different file: a craft pack's MCP mapping.
- **So a generated engine can already be used today, by hand.** Serve it with `python -m <pkg> --listen 127.0.0.1:7311`, then use `Engine › Attach…` in the editor or `plumcraft run wf.json --attach http://127.0.0.1:7311`. What is missing is starting it from PlumCraft.

### 1.4 Conformance

- **Every conformance suite is written for one engine.** `crates/text/tests/conformance.rs` (920 lines) runs its cases in mode 1, through a coordinator, and in modes 2 and 3 with the Rust and Python engines, but every graph names `text.*` nodes. The audio and craft suites are likewise their own.
- **Spec 19's `cargo xtask conformance --engine <manifest>` does not exist.** There is no `xtask` in this repository.

## 2. The generator: form and inputs

### 2.1 Recommendation: a subcommand of a new Python CLI in the SDK

```sh
uv run --project ~/workspace/plumcraft/python plumcraft-engine new ~/workspace/weather-engine \
    --id weather --name "Weather" --types text,table,array,svg
```

- **Where it lives:** a `plumcraft_engine.cli` module, with an entry point `plumcraft-engine` in `python/pyproject.toml`.
- **Subcommands:** `new` (the generator), `nodes`, `run` and `check` (section 5).
- **Templates:** package data under `python/src/plumcraft_engine/templates/engine/`, filled with `string.Template` from the standard library, so the generator adds no dependency.
- **Why Python:**
  - **The developer is a Python developer.** They need `uv` and the checkout, not a Rust build.
  - **The generated engine already depends on this package**, so `uv run plumcraft-engine run …` works inside the new project with nothing else installed.
  - **It is tested with pytest beside the SDK** that it scaffolds against, and it changes in the same commit when the SDK changes.

### 2.2 Alternatives

| Option | For | Against |
|---|---|---|
| `plumcraft-engine new` in the Python SDK (recommended) | No Rust build; one tool for scaffold, run and check; tested beside the SDK | Not in the editor's menus |
| `plumcraft new-engine` in the Rust CLI | One binary for everything | The Python developer needs a Rust build to start; templates as `include_str!` are awkward to edit |
| A copier template (`copier copy gh:…`) | `copier update` brings template changes into existing projects later | A new tool and template language; copier needs the template repository to be readable (it is private) |
| An editor command (`engine.new`, Engine › New Engine…) | Discoverable; "everything is a command" | A dialog for a developer tool. It can be added later as a command that calls the same Python entry point |

### 2.3 Arguments

| Argument | Meaning | Default |
|---|---|---|
| `DEST` | The new directory | Required |
| `--id ID` | Engine id, prefix of every node kind (`weather.shout`) | The last part of `DEST` without `-engine` |
| `--name NAME` | Display name | `ID` title-cased |
| `--types LIST` | Comma-separated types, from the table below | `text` |
| `--package NAME` | Python package | `<id>_engine` |
| `--sdk path\|git[@REV]` | How the project depends on the SDK (section 3.2) | `path` |
| `--python 3.X` | `.python-version` and `requires-python` | `3.12` |
| `--no-pycharm` | Skip `.idea/` and `.run/` | Off |
| `--git` | Run `git init` and make a first commit | Off |

`--types` values and what each one generates:

| Value | Wire type, as declared | Node value in Python | Example nodes | Extra dependency |
|---|---|---|---|---|
| `scalars` (always on) | `STRING INT FLOAT BOOL ENUM` | `str int float bool Literal` | `add` (two `Float`s with `Range`), `format_number` and `parse_number` | None |
| `text` | `TEXT`, exactly as `plumcraft_text` | `str` | `text` (source), `shout` (`Range`, `Literal`, `ConfigError`), `text_to_lines` and `lines_to_text` (with `list`) | None |
| `table` | `TABLE`, exactly as `plumcraft_text` | `{"columns": [{name, type, values}]}` | `sample_table`, `sort_rows`; `table_to_csv` and `csv_to_table` (with `text`); `table_to_array` and `array_to_table` (with `array`) | None |
| `array` | Core `ARRAY` (SDK `Array`) | ndarray or `ArrayValue` | `ramp` (an RGB gradient with `space` and channel names), `normalize` | numpy (already in the SDK) |
| `svg` | Core `SVG` (SDK `Svg`) | `Blob` made with `svg()` | `shapes` (circles from widgets), `recolor`; `svg_to_text` and `text_to_svg` (with `text`, else with `STRING`) | None |
| `image` | `IMAGE`, exactly as the craft adapter (`#2563eb`, media `image`) | `Blob` of PNG bytes | `checkerboard`; `image_to_array` and `array_to_image` (with `array`) | `pillow` |
| `list` | Any of the above with `list: true` | `List` | `split` and `join_list`, written in the low-level layer as the example of it | None |
| `any` | `ANY` and every building block | Any value | None: sets `building_blocks = True` | None |
| `audio` | Refused: "the Python SDK has no AUDIO values yet" until step 1b (section 8) adds them | | | |

- **Every engine also gets a `slow` node.** It passes its input through after `seconds`, calling `cx.progress` and stopping on `cx.cancelled()`. It works on the first chosen type and sits in the category `<Name> › Testing`. It shows progress and cancel, and the conformance checks need a node that takes a while (section 5.3).
- **Every conversion ships with its inverse**, following the maintainer's rule (memory: symmetric transforms). Each pair has a round-trip test (section 3.4).
- **A pair is generated only when both its types are chosen.**

### 2.4 What the generator refuses

Every refusal is an error message with exit code 2. Nothing is written until everything is valid. The project is built in a sibling temporary directory and renamed into place, so a failure leaves no partial directory.

- **`DEST` exists and is not empty.** There is no `--force`: overwriting a developer's work is never wanted.
- **A bad id.** The id must match `^[a-z][a-z0-9_]{0,31}$` and must not be:
  - a Python keyword;
  - a built-in engine id: `text`, `audio`, `array`, and every built-in craft pack id from `crates/craft/packs/*.toml`, a list the generator ships and a test keeps in sync;
  - a building-block prefix: `file`, `seq`, `vary`, `color`;
  - `plumcraft`, `core` or `x`.
- **A bad package name:** not an identifier, or one that shadows a standard-library or SDK module.
- **An unknown type, or `audio`,** with the list of valid types.
- **`--sdk path` when the SDK is not installed from a checkout.** The generator finds the checkout as `Path(plumcraft_engine.__file__).parents[2]`. If that directory has no `pyproject.toml` named `plumcraft-engine`, it suggests `--sdk git`.

## 3. The generated project

### 3.1 Layout

For `--id weather --types text,table,array,svg`:

```text
weather-engine/
|-- pyproject.toml            # weather-engine; depends on plumcraft-engine; ruff, pytest; [tool.plumcraft]
|-- uv.lock                   # written by the first `uv sync`; committed
|-- .python-version
|-- .gitignore
|-- README.md                 # for the developer: run, test, attach, debug, add a node
|-- plumcraft-engine.toml     # the engine manifest (spec 16.6), section 4
|-- src/weather_engine/
|   |-- __init__.py           # WeatherEngine: wires, building_blocks, nodes
|   |-- __main__.py           # sys.exit(WeatherEngine.serve())
|   `-- types.py              # the wire-type aliases (Text, Table), copied from the SDK's canonical ones
|-- tests/
|   |-- conftest.py           # an in-process Host fixture; a scratch root (python/tests/scratch.py, moved into the SDK in step 1)
|   |-- test_nodes.py         # each node called as a method: values, ConfigError, ranges
|   |-- test_roundtrips.py    # every conversion with its inverse
|   |-- test_engine.py        # schema matches tests/golden/schema.json; the demo runs, every node done
|   |-- test_conformance.py   # plumcraft_engine.conformance.check(WeatherEngine)
|   `-- golden/schema.json
|-- examples/demo.json        # a workflow that uses every node
|-- scripts/                  # serve, run, nodes, check, editor (section 5)
|-- .run/                     # PyCharm run configurations (section 6)
`-- .idea/                    # a minimal module file only (section 6)
```

- **`[tool.plumcraft] engine = "weather_engine:WeatherEngine"`** in `pyproject.toml` tells `plumcraft-engine run`, `nodes` and `check` which class to load, so the scripts take no arguments.
- **`[project.scripts] weather-engine = "weather_engine.__main__:main"`**, as `plumcraft-text-engine-py`.
- **Ruff:** the SDK's settings (`line-length = 160`, `select = ["E", "F", "W", "I", "B", "UP"]`), plus `ruff format`.
- **pytest:** `testpaths = ["tests"]`; the `slow` marker is registered and deselected by default.
- **`README.md`** covers:
  - the first five minutes: `uv sync`, `scripts/check`, `scripts/run`, opening the demo in the editor;
  - the dev loop: edit, Engine › Reconnect, run;
  - debugging in PyCharm (section 6.3);
  - how to add a node and how to add a wire type;
  - when to use the low-level layer;
  - a link to `python/README.md` for the full annotation reference.

### 3.2 How the project depends on the SDK

Recommendation: **an editable path dependency by default**, with `--sdk git` as the option for a machine without a checkout. Publishing to PyPI is a later choice for the maintainer.

```toml
[project]
dependencies = ["plumcraft-engine"]

[tool.uv.sources]
plumcraft-engine = { path = "/Users/<you>/workspace/plumcraft/python", editable = true }
# --sdk git: plumcraft-engine = { git = "ssh://git@github.com/seantrue/plumcraft", subdirectory = "python", rev = "<commit>" }
```

| Option | For | Against |
|---|---|---|
| Editable path (recommended) | The engine always runs the SDK of the checkout the editor is built from, so protocol changes can't drift apart; PyCharm navigates into SDK source; SDK fixes take effect without reinstalling | An absolute path in `pyproject.toml`, so a second machine needs the checkout in the same place, or the line edited |
| Git with `subdirectory` and a pinned `rev` | Reproducible; no local checkout needed | The repository is private, so it needs SSH access; every SDK fix needs a new `rev`; the wheel brings `plumcraft_text` along |
| PyPI (`plumcraft-engine`, free today) | The usual way for an outside developer | Needs a release process (the global release rule), a public licence decision, and probably splitting `plumcraft_text` out of the wheel; the SDK is changing daily (SVG this week) |

- **The generator writes the path that holds `pyproject.toml`** (`…/plumcraft/python`), not the repository root.
- **`README.md` explains how to switch** between path and git, so the choice isn't locked in.

### 3.3 The engine module

`src/<pkg>/__init__.py` is written to be read and copied:

- **Docstrings:** one for the module, and one per node, which becomes its description.
- **Clear sections:** the class attributes `id`, `name`, `version`, `category`, `family` and `wires`, then the sources, transforms and conversions, then `slow`.
- **Both error kinds are shown:** `ConfigError` (empty input, a bad setting) and `OpError` (a failure on this input).
- **Annotations:** `Range` with `invalid="clamp"` on one port, plus `Literal`, `Port(label=, tooltip=)` and a `@dataclass` parameter group, each on one node.
- **The low-level layer appears only for `list`:** one `Node` subclass with a `definition` dict and a comment explaining why.

### 3.4 Tests

- **`test_nodes.py`:** each node called as a plain method on an instance, without a host. It checks the outputs and that bad values raise `ConfigError` or `OpError`.
- **`test_roundtrips.py`:** for each conversion pair, `inverse(convert(x)) == x` on a few fixed values, and the reverse direction where it is exact. CSV through `TABLE` round-trips its typed columns.
- **`test_engine.py`:**
  - the catalogue equals `tests/golden/schema.json`, and `UPDATE_GOLDEN=1` rewrites it;
  - `examples/demo.json` runs in an in-process `Host` and every node ends `done`.

  The golden catches a renamed port, which would break saved workflows.
- **`test_conformance.py`:** `plumcraft_engine.conformance.check(Engine)` (section 5.3).

## 4. How PlumCraft loads and runs an out-of-tree engine

### 4.1 Recommendation: build spec 16.6's engine manifest, which is already normative

This is the smallest change that keeps the golden rules:

- **No engine in core (rule 3):** an engine is data in a file, and nothing in core names it.
- **No new protocol:** the manifest is local configuration, not a message.
- **Everything is a command (rule 4):** the Engine menu already switches by `engine.switch {engine, mode}`.

The generated `plumcraft-engine.toml`:

```toml
id = "weather"
name = "Weather"
mode = "process"                       # the default hosting; a command-started engine is also offered over HTTP
command = ["uv", "run", "--quiet", "--project", ".", "python", "-m", "weather_engine"]
cwd = "."                              # relative paths resolve against this file's real directory
hint = "cd ~/workspace/weather-engine && uv sync"
demo = "examples/demo.json"            # new field: opened when this engine is chosen with no workflow
restart = "on-crash"
max_restarts = 5

[conformance]                          # new table: what the generic suite needs
slow = "weather.slow"                  # a node that takes `seconds`, for the cancel and off-the-run-path cases
```

### 4.2 The changes, in order of dependence

1. **Parse the manifest in `plumcraft-proto`** (`manifest.rs`; serde and `toml`), so it joins the hostile-input sweep (`crates/proto/tests/hostile.rs`, rule 1):
   - files over a size cap are refused;
   - unknown keys are ignored (pipekit fields included);
   - an empty `command` is an error, as are `mode` values outside the closed set and an id that fails the generator's id rules (section 2.4).
2. **Resolve it in `plumcraft-host`** (`manifest::resolve`):
   - `~` is expanded;
   - relative `cwd` and `demo` are taken against the manifest's canonical directory, so a symlink in `~/.config/plumcraft/engines/` points back at the project;
   - the result is a `process::Manifest` (mode 2 or 3), or a URL and a token from `token_file` (attach).
   - **`uv` on a Finder-started editor's PATH:** a GUI app does not inherit the shell's PATH, so a bare `uv` (or `python`) in `command[0]` is looked up in `$PLUMCRAFT_UV`, then PATH, then `~/.local/bin`, `~/.cargo/bin` and `/opt/homebrew/bin`. A miss is an error that includes the manifest's `hint`.
   - `process::Manifest` gains `cwd`, which `ProcessHost::start` and `HttpHost::spawn` pass to `Command::current_dir`.
3. **Discovery in the editor and the CLI:**
   - `--engine-manifest FILE`, repeatable;
   - every `~/.config/plumcraft/engines/*.toml`;
   - an id that collides with a built-in engine, or with another manifest, is refused at load with both paths named, and the rest still load.
4. **Editor** (`apps/plumcraft-editor/src/main.rs`):
   - `EngineKind` gains `External(u16)`, an index into a registry built once at startup. `EngineKind` stays `Copy`, with no `Box::leak`.
   - `id()` and `label()` read the registry. `choices()` offers each external engine as `process` (mode 2) and `http` (mode 3, spawned with `--listen`); a manifest with `url` is offered as `attach`.
   - `--engine weather` and `--engine text,weather` (the coordinator) accept manifest ids.
   - `demo()` reads the manifest's `demo`, or opens an empty workflow.
   - **New command `engine.manifests.reload`** (Engine › Reload Engine List) reads the manifests again. A developer who installs a manifest doesn't restart the editor. The command gets a `panic_hunt` case like every command.
   - **The dev loop needs nothing new:** edit a node, then Engine › Reconnect (`engine.reconnect` starts a fresh process and keeps the graph), then Run.
5. **CLI** (`apps/plumcraft-cli`):
   - `plumcraft run WF --engine-manifest FILE [--http]` and `plumcraft nodes --engine-manifest FILE`. Without a workflow, `run` uses the manifest's `demo`.
   - `plumcraft conform --engine-manifest FILE` (section 7.2).
6. **`docs/interface.md`** (rule 5, same commit):
   - section 16.6 gains `demo`, `[conformance]`, `cwd` resolution against the manifest's real directory, the `uv` lookup, the rule that a command-started engine is offered over both mode 2 and mode 3, and the id-collision rule;
   - section 19 names `plumcraft conform --engine-manifest` in place of `cargo xtask conformance --engine`.

### 4.3 Installing a generated engine

- **`scripts/install`** symlinks `plumcraft-engine.toml` to `~/.config/plumcraft/engines/<id>.toml`; `scripts/uninstall` removes the link.
- **Without installing,** `scripts/editor` passes `--engine-manifest plumcraft-engine.toml`.

### 4.4 Alternatives

| Option | Against |
|---|---|
| `--engine-command "uv run …"` on the editor and the CLI | A second, ad-hoc form of what 16.6 already specifies; no name, demo or hint; can't appear in the Engine menu across restarts |
| Attach only (serve by hand, `Engine › Attach…`) | Works today with no change, and stays the PyCharm debugging path (section 6.3); but nothing supervises the engine (no restart, no orphan cleanup), and the developer starts two things by hand every time |
| A registry compiled into the editor | Breaks rule 3 and the out-of-tree goal |

## 5. Command-line scripts in the generated project

### 5.1 The scripts

All are `bash` with `set -euo pipefail`, run from the project root (macOS and Linux; Windows is not a target, spec 16.3). The Python-only ones need no Rust build.

| Script | Runs exactly | Needs |
|---|---|---|
| `scripts/check` | `uv run ruff check . && uv run ruff format --check . && uv run pytest` | uv |
| `scripts/nodes` | `uv run plumcraft-engine nodes`: the catalogue as a table (`id`, display name, inputs `name:TYPE`, outputs), in process | uv |
| `scripts/run [GRAPH] [--watch NODE.PORT]… [--cancel-after S] [--attach URL]` | `uv run plumcraft-engine run ${GRAPH:-examples/demo.json} "$@"`: runs the graph in an in-process `Host` (or over HTTP with `--attach`) and prints each event as it arrives, then the reply's values | uv |
| `scripts/serve [PORT]` | `uv run python -m <pkg> --listen 127.0.0.1:${PORT:-7311}`; it prints the attach URL. `PLUMCRAFT_ENGINE_TOKEN` is passed through when set | uv |
| `scripts/conform` | `uv run plumcraft-engine check`; then, if a `plumcraft` binary is found, `plumcraft conform --engine-manifest plumcraft-engine.toml` | uv; the Rust half needs a built CLI |
| `scripts/cli-run [GRAPH]` | `plumcraft run ${GRAPH:-examples/demo.json} --engine-manifest plumcraft-engine.toml "$@"`: the same run through the Rust host in mode 2 (`--http` for mode 3) | A built `plumcraft` |
| `scripts/editor [GRAPH]` | `plumcraft-editor ${GRAPH:-examples/demo.json} --engine-manifest plumcraft-engine.toml --engine <id>` | A built `plumcraft-editor` |
| `scripts/install`, `scripts/uninstall` | The manifest link (section 4.3) | |

`scripts/_plumcraft.sh` finds the Rust binaries, in this order:

1. `$PLUMCRAFT_BIN`;
2. PATH;
3. `$PLUMCRAFT_HOME/target/{release,debug}`, where `PLUMCRAFT_HOME` defaults to the SDK path's parent from `pyproject.toml`.

When it finds none, it prints the `cargo build --release -p plumcraft-cli -p plumcraft-editor` command and exits 2.

### 5.2 `plumcraft-engine run` output

The output matches `plumcraft run` closely enough that one person can read both:

```text
(weather 0.1.0, in process; 9 nodes)
     12  story      started
     14  slow       progress 0.40 40%
     31  shout      done      3 ms   TEXT "A CORGI NAPS…" (34 chars)
     33  ramp       done     11 ms   ARRAY uint8 (64, 64, 3) rgb, blob md5:nd1:4f2a…
     35  broken     error     op: this node always fails
     36  after      skipped
run r1: 7 done, 1 error, 1 skipped in 2.04 s
```

- **`--raw`** prints each event as JSON, one per line.
- **The exit code** is 0 when every node is `done` or `cached`, and 1 otherwise. A script can then use `run` as a smoke test.

### 5.3 `plumcraft-engine check`: the Python half of conformance

`plumcraft_engine.conformance` is engine-agnostic: it reads the catalogue and the `[conformance]` table, never node names. It runs each case in process and again against a spawned HTTP front, which is the same command that `HttpHost::spawn` uses.

- **Session:**
  - `hello` negotiates protocol 2 and refuses protocol 3;
  - `schema` parses, and the same digest gives `unchanged`;
  - every wire type a node uses is declared;
  - every node id starts with `<id>.`.
- **Core types:** `ARRAY` and `SVG`, when used, equal `core_types` exactly (as `test_the_svg_type_is_the_core_one`).
- **Every node with defaults runs:** a node whose inputs all have defaults ends in a terminal state, never with an `engine` error.
- **The demo:**
  - envelope `seq` rises;
  - every node ends terminal;
  - `preview` comes before any downstream `started`;
  - every `done` carries `ms`;
  - a second run is `cached` for nodes whose memo scope isn't `none`.
- **Cancel** (needs `[conformance] slow`):
  - mid-run, unreached nodes are `cancelled` within 500 ms;
  - a cancel sent before its run ends it `cancelled`;
  - during a 5 s run, `run.get`, `events.poll` and `ping` each answer within 200 ms (HTTP).
- **Hostile parameters:** each node gets wrong types, NaN, values out of range, empty strings and huge strings. It must fail as `config` or `op` and the engine must keep running. This is the Python form of `panic_hunt`.

The Rust half (section 7.2) adds what only a real binding can show: mode 2 `kill -9` recovery, the same-host proof failing closed, wrong tokens, and SSE resume.

## 6. PyCharm

### 6.1 What to generate

- **`.run/*.run.xml`**, PyCharm's shared run configurations ("Store as project file"; they belong in version control). All use `SDK_HOME = $PROJECT_DIR$/.venv/bin/python` and `WORKING_DIRECTORY = $PROJECT_DIR$`, so they work on any machine after `uv sync`:

| Configuration | Type | What it runs |
|---|---|---|
| Serve (HTTP 7311) | Python, module mode | `-m <pkg> --listen 127.0.0.1:7311`; run it under Debug to stop at breakpoints in nodes (section 6.3) |
| Run demo | Python, module mode | `-m plumcraft_engine.cli run examples/demo.json` |
| Run demo (attached) | Python, module mode | The same with `--attach http://127.0.0.1:7311`; pairs with Serve |
| Nodes | Python, module mode | `-m plumcraft_engine.cli nodes` |
| Tests | pytest | `$PROJECT_DIR$/tests` |
| Conformance | pytest | `tests/test_conformance.py` |
| Editor (manifest) | Shell Script | `scripts/editor` |

- **A minimal `.idea/`:** `<id>-engine.iml` with `src` as a source folder, `tests` as a test folder and `.venv` excluded; `modules.xml`; `.idea/.gitignore` (`workspace.xml`, `shelf/`, `misc.xml`, `dataSources*`). The repository's own `.idea/plumcraft-engine.iml` has the same shape.
- **Leave out `misc.xml`.** It names the project interpreter by a machine-local SDK name.
- **Leave out the test-runner choice.** PyCharm picks pytest when it is installed.

### 6.2 What PyCharm needs that isn't in the project

- **PyCharm 2024.3 or later,** which detects a `uv.lock` and offers to create the uv environment on open. With an older PyCharm, run `uv sync` first and add `.venv/bin/python` as an existing interpreter. `README.md` gives both.
- **`uv` on PATH,** for the scripts and the manifest command.
- **For the Rust-side scripts,** the PlumCraft checkout built (`plumcraft`, `plumcraft-editor`).

### 6.3 Debugging a node in PyCharm

1. Debug "Serve (HTTP 7311)".
2. In the editor, Engine › Attach… `http://127.0.0.1:7311` with an empty token (loopback only).
3. Run, and the breakpoint in the node method stops the run.

- **The run is not lost while paused.** The host answers `run.get` and `events.poll` off the run path, so the editor shows the node as running and doesn't time it out. Spec 12.3 inactivity applies after `inactivity_s`. `README.md` notes that a pause longer than that gets a synthesised failure, which a later real reply replaces.
- **Alternative: attach to the editor's own engine.** The editor can start the engine from the manifest, and then PyCharm's "Attach to Process" (Run › Attach to Process, which needs the PyCharm debugger helpers) attaches to it. It is noted in `README.md` but is not the default.

### 6.4 `.gitignore`

```text
.venv/
__pycache__/
*.pyc
.pytest_cache/
.ruff_cache/
.idea/workspace.xml
.idea/misc.xml
.idea/shelf/
dist/
build/
*.egg-info/
.DS_Store
.engine-token
```

`uv.lock`, `.run/`, `.idea/<id>-engine.iml` and `.idea/modules.xml` are committed.

## 7. Testing the generator

### 7.1 In the SDK (`python/tests/test_new_engine.py`)

| Test | Checks |
|---|---|
| Refusals | Each rule in section 2.4 gives exit 2 with its message and writes nothing (`DEST` absent afterwards; no stray temporary directory) |
| Golden output | Three cases (`scalars`; `text,table`; every type but `audio`) are generated into `scratch.mkdtemp()` and compared file for file with `python/tests/golden/new_engine/<case>/`. The SDK path is normalised to `{{SDK}}`. `UPDATE_GOLDEN=1` rewrites them |
| Well-formed output | Every `.py` compiles; `ruff check` and `ruff format --check` pass on the output; `pyproject.toml` and the manifest parse (`tomllib`); every `.run/*.run.xml` and `.idea/*` file parses as XML |
| It works without uv | The generated package is imported with `src` on `sys.path`: the class compiles, `Host(Engine)` serves its schema, the demo runs with every node `done`, and the generated `tests/` pass through `pytest.main` in a subprocess |
| Pairs | For the all-types case, every conversion node has its inverse in the catalogue (a check that reads node ids) |

### 7.2 Generic Rust conformance, and the end-to-end test

- **A new crate `crates/conform` (`plumcraft-conform`)** holds the cases of `crates/text/tests/conformance.rs` that need only a chain and a slow node. They are rewritten over a `Fixture {connect, chain_graph, slow_kind}` and built from a manifest and the engine's own catalogue:
  - hello and schema;
  - record invariants;
  - every delivery reduces the same;
  - resume from seq;
  - cancel mid-run and before the run;
  - off the run path;
  - memo reuse;
  - mode 2 `kill -9`;
  - a wrong token is refused;
  - an engine that cannot start is down, not hung.

  `plumcraft conform --engine-manifest FILE [--modes process,http]` runs them. The text engine's suite then calls the generic cases with its own fixture, which proves the extraction loses nothing. Its text-specific cases (data values, Rust and Python parity) stay where they are.
- **End to end** (`apps/plumcraft-cli/tests/generated_engine.rs`; it needs uv, as the existing Python conformance cases already do):
  1. generate the all-types engine into the test's scratch root with `uv run --project python plumcraft-engine new`;
  2. `uv sync` and `uv run pytest` inside it;
  3. `plumcraft conform` against its manifest in modes 2 and 3;
  4. `plumcraft run` of its demo, with every node `done`.
- **Editor kittest:** load the generated manifest with `--engine-manifest`, call `engine.switch` to it, and expect the demo to run with every node done. This follows `attach_connects_to_a_running_http_engine`.

## 8. Order of work

| Step | What | Check |
|---|---|---|
| 1 | **SDK groundwork.** Canonical `Text`, `Table` and `Image` aliases in the SDK (not core types; `plumcraft_text` imports them); an `image()` constructor; `python/tests/scratch.py` moved into the SDK as `plumcraft_engine.scratch`; a test that an `Svg` input receives a `Blob` | `python/tests/golden/text_schema.json` unchanged; new tests for `Svg` and `Image` inputs |
| 1b | Optional, maintainer's call: `Audio` in the Python SDK, the same `AUDIO` and `Form::Audio` as `crates/audio` | Matches the Rust engine's wire type and handle for one tone |
| 2 | `plumcraft_engine.cli` with `nodes` and `run` (in process and `--attach`); `[tool.plumcraft] engine` lookup | pytest drives the text engine through both; the exit code follows node states |
| 3 | `plumcraft_engine.conformance` and `check` | Passes on `TextEngine`; fails, naming the case, on a deliberately broken engine (a node that sleeps through cancel, a node id without the prefix) |
| 4 | The generator, `new`: scalars and text, refusals, the atomic write, golden output | Section 7.1 tests for that type set |
| 5 | The remaining types and their conversion pairs, and the `list` low-level example | Golden output for all types; round-trip tests pass inside the generated project |
| 6 | PyCharm files and scripts | XML parses; a P-checklist (`docs/acceptance/P1x-engine-generator.md`): open in PyCharm, run every configuration, stop at a breakpoint over Attach |
| 7 | The manifest parser in `plumcraft-proto`, resolution in `plumcraft-host` (`cwd`, the `uv` lookup); `docs/interface.md` 16.6 | Unit tests; the hostile-input sweep; schema regenerated if any type is exported |
| 8 | CLI `--engine-manifest` for `run` and `nodes` | An integration test with a manifest for the Python text engine, then for a generated engine |
| 9 | Editor: `--engine-manifest`, `~/.config/plumcraft/engines/`, `EngineKind::External`, Engine menu entries, `engine.manifests.reload`, `demo` | Kittest: switch to a manifest engine and run the demo; `panic_hunt` for the new command; id collisions refused |
| 10 | `crates/conform` and `plumcraft conform`; the text suite moved onto it; spec 19 updated | The text suite is still green with the same case count; the generated engine passes in modes 2 and 3 |
| 11 | The end-to-end test; `README.md` in the generated project reviewed; `docs/STATUS.md`; AI.md at commit | `cargo test --all-features`, clippy and the Python gates (AGENTS.md) |

Steps 1 to 6 need no Rust and give a working out-of-tree engine that the editor can already reach through Attach. Steps 7 to 10 let PlumCraft start it.

**Progress (2026-10-30):** all eleven steps are built (`docs/STATUS.md`). What is left is the maintainer's: the checklist below. The P-checklist is `docs/acceptance/P09-engine-generator.md`. Step 1 went further than planned: TEXT, TABLE, IMAGE and AUDIO became core types (decision G3), so the SDK's `Text`, `Table`, `Image` and `Audio` are the core declarations, not copies.

## 9. Decisions

The maintainer, 2026-10-30: "G1,G2 as recc / G3, yes core data types / G4, yes, audio / G5-G9 as recc", then "Core audio, please".

| # | Question | Recommendation | Decision (maintainer, 2026-10-30) |
|---|---|---|---|
| G1 | SDK distribution: path, git, or publish `plumcraft-engine` to PyPI (and split `plumcraft_text` out of the wheel) | Path now, git as an option; PyPI when the SDK settles | **Decided:** as recommended |
| G2 | Generator form: Python SDK CLI, Rust CLI, or a copier template (which allows `copier update`) | Python SDK CLI | **Decided:** as recommended |
| G3 | Promote `TEXT` and `TABLE` (and `IMAGE`) to core types, as `ARRAY` and `SVG` were | Not needed for the generator (same media already crosses), but it would make the canonical declarations normative | **Decided: yes**, TEXT, TABLE and IMAGE become core types |
| G4 | `AUDIO` in the Python SDK now (step 1b) or later | Later, unless an audio engine is wanted soon | **Decided: yes**, AUDIO in the Python SDK now (step 1b), and AUDIO a core type too ("Core audio, please") |
| G5 | Manifest command: `uv run --project .` (stays in sync with `pyproject.toml`; needs `uv` found from a GUI app) or `.venv/bin/python -m <pkg>` (faster start; needs `uv sync` after every dependency change) | `uv run`, with the lookup in section 4.2 | **Decided:** as recommended |
| G6 | Package naming: `<id>_engine` or `plumcraft_<id>` | `<id>_engine`: out-of-tree engines don't claim the `plumcraft_` namespace | **Decided:** as recommended |
| G7 | Commit the minimal `.idea/` or only `.run/` | Both, as listed in section 6.4 | **Decided:** as recommended |
| G8 | Development HTTP serving on loopback without a token, or a generated `.engine-token` | No token on loopback (the SDK already refuses non-loopback without one) | **Decided:** as recommended |
| G9 | Manifest-started engines offered over HTTP as well as mode 2 | Yes: the SDK's `serve()` handles both, and mode 3 is what a remote run will use | **Decided:** as recommended |
