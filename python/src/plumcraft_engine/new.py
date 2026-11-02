"""`plumcraft-engine new`: a new engine as a standalone project, for a developer to take over
(`docs/research/17-engine-generator.md`, sections 2 and 3).

    plumcraft-engine new DEST [--id ID] [--name NAME] [--types text,table,array,…] [--package NAME]
                              [--sdk pypi|path|git[@REV]] [--python 3.X] [--no-pycharm] [--git]

Everything is checked before anything is written; a refusal is a message and exit 2. The project
is built in a sibling temporary directory and renamed into place, so a failure leaves nothing.
Files come from `templates/engine/` through `string.Template`, so the generator adds no dependency.
"""

from __future__ import annotations

import argparse
import json
import keyword
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from importlib import resources
from pathlib import Path
from string import Template

# The types `--types` takes, in the order their nodes are written. `scalars` are always on.
TYPES = ("scalars", "text", "table", "array", "svg", "image", "audio", "json", "list", "any")
# Conversions between two types, generated only when both are chosen; each is a pair of inverses.
PAIRS = (("table", "text"), ("table", "array"), ("svg", "text"), ("image", "array"), ("audio", "array"), ("json", "text"))
# What each type's nodes import: standard modules, third-party modules, and names from the SDK.
NEEDS: dict = {
    "scalars": ({"time"}, set(), {"ConfigError", "Cx", "Engine", "Float", "NodeError", "Port", "Range", "String", "node"}),
    "text": (set(), set(), {"OpError", "Text"}),
    "table": (set(), set(), {"Table"}),
    "array": (set(), {"numpy"}, {"Array", "ArrayValue"}),
    "svg": ({"re"}, set(), {"Svg", "svg"}),
    "image": ({"io"}, {"numpy", "pil"}, {"Blob", "Image", "OpError", "image"}),
    "audio": (set(), {"numpy"}, {"Audio", "AudioValue"}),
    "json": (set(), set(), {"Json"}),
    "list": (set(), set(), {"List", "Node"}),
    "any": (set(), set(), set()),
    ("table", "text"): ({"csv", "io"}, set(), set()),
    ("json", "text"): ({"json"}, set(), set()),
}
# Ids a new engine can't take: the built-in engines, the craft packs (crates/craft/packs/*.toml,
# a test keeps this in step), the building blocks' prefixes, and the reserved.
BUILTIN = ("text", "audio", "array")
CRAFTS = tuple(f"{c}craft" for c in ("cad", "deck", "design", "effect", "fig", "film", "grid", "pdf", "photo", "solid", "sound", "vector", "word"))
PREFIXES = ("file", "seq", "vary", "color")
RESERVED = ("plumcraft", "core", "x")
# The project's scripts; `_plumcraft.sh` is sourced, not run.
SCRIPTS = ("check", "nodes", "run", "serve", "conform", "cli-run", "editor", "install", "uninstall", "_plumcraft.sh")
ID = re.compile(r"^[a-z][a-z0-9_]{0,31}$")
GIT_URL = "ssh://git@github.com/seantrue/plumcraft"


class Refused(Exception):
    """Something the generator won't do: its message, and exit 2."""


@dataclass
class Plan:
    """What to generate, checked."""

    dest: Path
    id: str
    name: str
    types: tuple[str, ...]
    package: str
    sdk_source: str
    python: str
    pycharm: bool = True

    @property
    def cls(self) -> str:
        return "".join(p.capitalize() for p in self.id.split("_")) + "Engine"

    @property
    def project(self) -> str:
        return f"{self.id.replace('_', '-')}-engine"

    def values(self) -> dict[str, str]:
        v = {k: getattr(self, k) for k in ("id", "name", "package", "cls", "project", "python", "sdk_source")}
        watch = "loud.text" if "text" in self.types else "parsed.float"
        deps = ['"plumcraft-engine>=0.1,<0.2"', *(['"pillow>=10"'] if "image" in self.types else [])]
        sources = ""
        if self.sdk_source:
            sources = f"# Where the PlumCraft SDK comes from; README.md says how to switch.\n[tool.uv.sources]\nplumcraft-engine = {self.sdk_source}\n\n"
        extra = {"python_tag": self.python.replace(".", ""), "types": ", ".join(self.types), "watch": watch, "dependencies": ", ".join(deps)}
        return v | extra | {"uv_sources": sources}

    @property
    def pairs(self) -> list[tuple[str, str]]:
        return [q for q in PAIRS if q[0] in self.types and q[1] in self.types]


# ---------- checking ----------


def check_id(i: str) -> None:
    if not ID.match(i):
        raise Refused(f"--id {i!r}: lower case letters, digits and _, starting with a letter, at most 32")
    if keyword.iskeyword(i):
        raise Refused(f"--id {i!r} is a Python keyword")
    if i in BUILTIN or i in CRAFTS:
        raise Refused(f"--id {i!r} is a built-in engine")
    if i in PREFIXES:
        raise Refused(f"--id {i!r} is a building blocks' prefix")
    if i in RESERVED:
        raise Refused(f"--id {i!r} is reserved")


def check_package(p: str) -> None:
    if not p.isidentifier() or keyword.iskeyword(p):
        raise Refused(f"--package {p!r} isn't a Python package name")
    if p in sys.stdlib_module_names or p in ("plumcraft_engine", "plumcraft_text"):
        raise Refused(f"--package {p!r} would shadow a module of that name")


def check_types(raw: str) -> tuple[str, ...]:
    asked = [t.strip() for t in raw.split(",") if t.strip()]
    for t in asked:
        if t not in TYPES:
            raise Refused(f"--types {t}: unknown (the types are: {', '.join(TYPES)})")
    wanted = {"scalars", *asked}
    if "list" in wanted:  # a list here is a list of texts
        wanted.add("text")
    return tuple(t for t in TYPES if t in wanted)


def checkout() -> Path | None:
    """The SDK's folder in a PlumCraft checkout, or None when it isn't installed from one."""
    import plumcraft_engine

    root = Path(plumcraft_engine.__file__).resolve().parents[2]
    try:
        return root if 'name = "plumcraft-engine"' in (root / "pyproject.toml").read_text("utf-8") else None
    except OSError:
        return None


def sdk_source(sdk: str) -> str:
    """The `[tool.uv.sources]` value for `--sdk`, or nothing for the package on PyPI."""
    root = checkout()
    if sdk == "pypi":
        return ""
    if sdk == "path":
        if root is None:
            raise Refused("--sdk path: this SDK isn't installed from a PlumCraft checkout; use --sdk pypi or git@REV")
        return f'{{ path = "{root}", editable = true }}'
    if sdk == "git" or sdk.startswith("git@"):
        rev = sdk.partition("@")[2]
        if not rev:
            out = subprocess.run(["git", "-C", str(root or "."), "rev-parse", "HEAD"], capture_output=True, text=True, check=False)
            rev = out.stdout.strip() if out.returncode == 0 else ""
        if not rev:
            raise Refused("--sdk git: no commit to pin; use --sdk git@REV")
        return f'{{ git = "{GIT_URL}", subdirectory = "python", rev = "{rev}" }}'
    raise Refused(f"--sdk {sdk!r}: pypi, path, git or git@REV")


def check_dest(dest: Path) -> None:
    if dest.is_symlink() or (dest.exists() and not dest.is_dir()):
        raise Refused(f"{dest} exists and isn't a directory")
    if dest.exists() and any(dest.iterdir()):
        raise Refused(f"{dest} exists and isn't empty")


def plan(args: argparse.Namespace) -> Plan:
    dest = Path(args.dest).expanduser().absolute()
    i = args.id or re.sub(r"-engine$", "", dest.name).replace("-", "_")
    check_id(i)
    package = args.package or f"{i}_engine"
    check_package(package)
    if not re.fullmatch(r"3\.\d+", args.python) or int(args.python.split(".")[1]) < 11:
        raise Refused(f"--python {args.python}: 3.11 or later, as 3.12")
    types = check_types(args.types)
    source = sdk_source(args.sdk or ("path" if checkout() else "pypi"))
    check_dest(dest)
    return Plan(dest, i, args.name or i.replace("_", " ").title(), types, package, source, args.python, not args.no_pycharm)


# ---------- writing ----------


def template(name: str) -> Template:
    return Template(resources.files("plumcraft_engine").joinpath("templates", "engine", name).read_text("utf-8"))


def fragment(p: Plan, name: str) -> str:
    return template(f"fragments/{name}.py.tmpl").substitute(p.values())


def exists(name: str) -> bool:
    return resources.files("plumcraft_engine").joinpath("templates", "engine", "fragments", f"{name}.py.tmpl").is_file()


def by_type(p: Plan, prefix: str) -> str:
    """Each chosen type's fragment `<prefix><type>` that exists, then each pair's `<prefix><a>_<b>`."""
    names = [f"{prefix}{t}" for t in p.types] + [f"{prefix}{a}_{b}" for a, b in p.pairs]
    return "".join(fragment(p, n) for n in names if exists(n))


def imports(std: set[str], third: set[str], names: set[str], first: str = "", typing_names: str = "") -> str:
    """An import block as ruff sorts it. `third` holds `numpy`, `pil` and `pytest`."""
    blocks = []
    head = [f"import {m}" for m in sorted(std)] + ([f"from typing import {typing_names}"] if typing_names else [])
    if head:
        blocks.append("\n".join(head))
    mid = []
    if "numpy" in third:
        mid.append("import numpy as np")
    if "pytest" in third:
        mid.append("import pytest")
    if "pil" in third:
        mid.append("from PIL import Image as Pil")
    if names:
        ordered = sorted(names, key=lambda n: (n[0].islower(), n))
        line = f"from plumcraft_engine import {', '.join(ordered)}"
        mid.append(line if len(line) <= 160 else "from plumcraft_engine import (\n" + "".join(f"    {n},\n" for n in ordered) + ")")
    if mid:
        blocks.append("\n".join(mid))
    if first:
        blocks.append(first)
    return "\n\n".join(blocks)


def engine_source(p: Plan) -> str:
    std, third, names = set(), set(), set()
    for k in [*p.types, *p.pairs]:
        a, b, c = NEEDS.get(k, (set(), set(), set()))
        std |= a
        third |= b
        names |= c
    typing_names = "Annotated, Literal" if "text" in p.types else "Annotated"
    helpers = "".join(fragment(p, f"helpers_{t}") for t in p.types if exists(f"helpers_{t}"))
    attrs = ""
    if "image" in p.types:
        attrs += '    # Their versions join the memo salt: an upgrade never serves an old result.\n    dependencies = ("pillow",)\n'
    if "any" in p.types:
        attrs += "    # Every building block (Vary, sequences, files), as ANY ports.\n    building_blocks = True\n"
    if "list" in p.types:
        attrs += "\n    def __init__(self, info: dict | None = None):\n        super().__init__(info)\n        self.with_node(Split()).with_node(JoinList())\n"
    nodes = by_type(p, "") + fragment(p, "slow_text" if "text" in p.types else "slow_scalars")
    head = imports(std, third, names, typing_names=typing_names)
    return template("engine.py.tmpl").substitute(p.values() | {"imports": head, "helpers": helpers, "attrs": attrs, "nodes": nodes})


def demo(p: Plan) -> dict:
    """A workflow that uses every node."""

    def wire(a: str, ap: str, b: str, bp: str) -> dict:
        return {"from": {"node": a, "port": ap}, "to": {"node": b, "port": bp}}

    has = p.types.__contains__
    i = p.id
    nodes = {
        "sum": {"kind": f"{i}.add", "values": {"a": 2.0, "b": 0.5}},
        "formatted": {"kind": f"{i}.format_number", "values": {"decimals": 3}},
        "parsed": {"kind": f"{i}.parse_number"},
    }
    wires = [wire("sum", "float", "formatted", "value"), wire("formatted", "string", "parsed", "string")]

    def add(name: str, kind: str, values: dict | None = None) -> None:
        nodes[name] = {"kind": f"{i}.{kind}", **({"values": values} if values else {})}

    if has("text"):
        add("source", "text", {"text": "a corgi naps"})
        add("loud", "shout", {"marks": 3})
        add("hex", "text_to_hex")
        add("back", "hex_to_text")
        add("waited", "slow")
        wires += [wire("source", "text", "loud", "text"), wire("loud", "text", "hex", "text")]
        wires += [wire("hex", "string", "back", "string"), wire("back", "text", "waited", "text")]
    else:
        add("waited", "slow")
        wires.append(wire("parsed", "float", "waited", "value"))
    if has("table"):
        add("sample", "sample_table")
        add("sorted", "sort_rows", {"descending": True})
        wires.append(wire("sample", "table", "sorted", "table"))
    if has("table") and has("text"):
        add("csv", "table_to_csv")
        add("parsed_csv", "csv_to_table")
        wires += [wire("sorted", "table", "csv", "table"), wire("csv", "text", "parsed_csv", "text")]
    if has("array"):
        add("gradient", "ramp")
        add("unit", "normalize")
        wires.append(wire("gradient", "array", "unit", "array"))
    if has("table") and has("array"):
        add("numbers", "table_to_array")
        add("numbers_back", "array_to_table")
        wires += [wire("sorted", "table", "numbers", "table"), wire("numbers", "array", "numbers_back", "array")]
    if has("svg"):
        add("drawing", "shapes")
        add("green", "recolor")
        wires.append(wire("drawing", "svg", "green", "drawing"))
    if has("svg") and has("text"):
        add("svg_source", "svg_to_text")
        add("svg_back", "text_to_svg")
        wires += [wire("green", "svg", "svg_source", "drawing"), wire("svg_source", "text", "svg_back", "text")]
    if has("image"):
        add("board", "checkerboard")
        add("board_size", "image_size")
        wires.append(wire("board", "image", "board_size", "picture"))
    if has("image") and has("array"):
        add("pixels", "image_to_array")
        add("repainted", "array_to_image")
        wires += [wire("board", "image", "pixels", "picture"), wire("pixels", "array", "repainted", "array")]
    if has("audio"):
        add("beep", "tone")
        add("quiet", "gain")
        wires.append(wire("beep", "audio", "quiet", "audio"))
    if has("audio") and has("array"):
        add("samples", "audio_to_array")
        add("played", "array_to_audio")
        wires += [wire("quiet", "audio", "samples", "audio"), wire("samples", "array", "played", "array")]
    if has("json"):
        add("settings", "settings")
        add("name", "get_key")
        wires.append(wire("settings", "json", "name", "value"))
    if has("json") and has("text"):
        add("as_text", "json_to_text")
        add("as_json", "text_to_json")
        wires += [wire("settings", "json", "as_text", "value"), wire("as_text", "text", "as_json", "text")]
    if has("list"):
        add("words", "split")
        add("sentence", "join_list")
        wires += [wire("source", "text", "words", "text"), wire("words", "items", "sentence", "items")]
    return {"version": 1, "nodes": nodes, "wires": wires}


def write_files(p: Plan, root: Path) -> None:
    v = p.values()
    listed = f"from {p.package} import JoinList, Split" if "list" in p.types else ""
    node_names = {"ConfigError", *(["OpError"] if "text" in p.types else [])}
    node_imports = imports(set(), {"pytest", *(["numpy"] if "array" in p.types else [])}, node_names, listed)
    pair_imports = imports(set(), {"pytest"}, set(), listed)
    files = {
        "pyproject.toml": template("pyproject.toml.tmpl").substitute(v),
        ".python-version": p.python + "\n",
        ".gitignore": template("gitignore.tmpl").substitute(v),
        "README.md": template("README.md.tmpl").substitute(v),
        "plumcraft-engine.toml": template("plumcraft-engine.toml.tmpl").substitute(v),
        f"src/{p.package}/__init__.py": engine_source(p),
        f"src/{p.package}/__main__.py": template("main.py.tmpl").substitute(v),
        "tests/conftest.py": template("conftest.py.tmpl").substitute(v),
        "tests/test_engine.py": template("test_engine.py.tmpl").substitute(v),
        "tests/test_conformance.py": template("test_conformance.py.tmpl").substitute(v),
        "tests/test_nodes.py": template("test_nodes.py.tmpl").substitute(v | {"imports": node_imports, "nodes": by_type(p, "test_nodes_")}),
        "tests/test_roundtrips.py": template("test_roundtrips.py.tmpl").substitute(v | {"imports": pair_imports, "pairs": by_type(p, "test_pairs_")}),
        "examples/demo.json": json.dumps(demo(p), indent=2) + "\n",
    }
    files |= {f"scripts/{name}": template(f"scripts/{name}.tmpl").substitute(v) for name in SCRIPTS}
    if p.pycharm:
        files |= pycharm_files(p)
    for rel, text in files.items():
        f = root / rel
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text, "utf-8")
        if rel.startswith("scripts/") and not rel.endswith(".sh"):
            f.chmod(0o755)
    golden = root / "tests" / "golden" / "schema.json"
    golden.parent.mkdir()
    golden.write_text(catalogue(p, root), "utf-8")


def pycharm_files(p: Plan) -> dict[str, str]:
    """Shared run configurations in `.run/`, using the project's `.venv`, and a minimal `.idea/`:
    no `misc.xml`, which names a machine's interpreter (`docs/research/17-engine-generator.md`, 6.1)."""
    v = p.values()
    attached = "run examples/demo.json --attach http://127.0.0.1:7311"
    modules = (
        ("Serve (HTTP 7311)", p.package, "--listen 127.0.0.1:7311 --token-file .engine-token"),
        ("Run demo", "plumcraft_engine.cli", "run examples/demo.json"),
        ("Run demo (attached)", "plumcraft_engine.cli", attached),
        ("Nodes", "plumcraft_engine.cli", "nodes"),
    )
    tests = (("Tests", "tests"), ("Conformance", "tests/test_conformance.py"))
    files = {}
    for title, module, parameters in modules:
        files[f".run/{title}.run.xml"] = template("pycharm/module.run.xml.tmpl").substitute(v | {"title": title, "module": module, "parameters": parameters})
    for title, target in tests:
        files[f".run/{title}.run.xml"] = template("pycharm/pytest.run.xml.tmpl").substitute(v | {"title": title, "target": target})
    files[".run/Editor.run.xml"] = template("pycharm/shell.run.xml.tmpl").substitute(v | {"title": "Editor", "script": "scripts/editor"})
    files[f".idea/{p.project}.iml"] = template("pycharm/module.iml.tmpl").substitute(v)
    files[".idea/modules.xml"] = template("pycharm/modules.xml.tmpl").substitute(v)
    files[".idea/.gitignore"] = template("pycharm/idea.gitignore.tmpl").substitute(v)
    return files


def catalogue(p: Plan, root: Path) -> str:
    """The golden catalogue, from the engine just written, in a process of its own."""
    code = (
        "import json, sys\n"
        f"sys.path.insert(0, 'src')\nfrom {p.package} import {p.cls}\n"
        f"s = {p.cls}().schema()\n"
        "print(json.dumps({'wire_types': s['wire_types'], 'nodes': s['nodes']}, indent=2, sort_keys=True))\n"
    )
    out = subprocess.run([sys.executable, "-B", "-c", code], cwd=root, capture_output=True, text=True, check=False)
    if out.returncode != 0:
        raise Refused(f"the generated engine doesn't load (a bug in the generator): {out.stderr.strip()[-400:]}")
    return out.stdout


def generate(p: Plan) -> None:
    """Build beside the destination, then rename into place."""
    parent = p.dest.parent
    parent.mkdir(parents=True, exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix=f".{p.dest.name}.", dir=parent))
    try:
        write_files(p, tmp)
        tmp.chmod(0o755)
        if p.dest.exists():
            p.dest.rmdir()
        os.rename(tmp, p.dest)
    except BaseException:
        shutil.rmtree(tmp, ignore_errors=True)
        raise


def first_commit(dest: Path) -> None:
    for cmd in (["git", "init", "-q"], ["git", "add", "-A"], ["git", "commit", "-q", "-m", "A new PlumCraft engine, from plumcraft-engine new"]):
        out = subprocess.run(cmd, cwd=dest, capture_output=True, text=True, check=False)
        if out.returncode != 0:
            print(f"plumcraft-engine: the project is written, but {' '.join(cmd)} failed: {out.stderr.strip()}", file=sys.stderr)
            return


def parser() -> argparse.ArgumentParser:
    a = argparse.ArgumentParser(prog="plumcraft-engine new", description="A new PlumCraft engine as a standalone project.")
    a.add_argument("dest", help="the new directory")
    a.add_argument("--id", help="engine id, the prefix of every node kind (default: from DEST, without -engine)")
    a.add_argument("--name", help="display name (default: the id, title-cased)")
    a.add_argument("--types", default="text", help=f"comma-separated: {', '.join(TYPES)} (default: text; scalars are always on)")
    a.add_argument("--package", help="Python package (default: <id>_engine)")
    a.add_argument("--sdk", help="where the SDK comes from: pypi, path, git or git@REV (default: path in a checkout, else pypi)")
    a.add_argument("--python", default="3.12", help="Python version (default: 3.12)")
    a.add_argument("--no-pycharm", action="store_true", help="leave out .idea/ and .run/")
    a.add_argument("--git", action="store_true", help="run git init and make a first commit")
    return a


def main(argv: list[str]) -> int:
    try:
        args = parser().parse_args(argv)
        p = plan(args)
        generate(p)
    except SystemExit as e:  # argparse: its message is out
        return e.code if isinstance(e.code, int) else 2
    except Refused as e:
        print(f"plumcraft-engine new: {e}", file=sys.stderr)
        return 2
    if args.git:
        first_commit(p.dest)
    print(f"{p.name}: a new engine in {p.dest}\n")
    print(f"  cd {p.dest}\n  uv sync\n  scripts/check\n  scripts/run --watch {p.values()['watch']}")
    return 0
