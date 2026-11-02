"""`plumcraft-engine new`: what it refuses (writing nothing), its output file for file against the
goldens (`UPDATE_GOLDEN=1` rewrites them), and that the output is a working project: it compiles,
passes ruff with its own settings, its own tests pass, for every type alone and every pair, and its
scripts and PyCharm files are well-formed."""

import json
import os
import re
import subprocess
import sys
import tomllib
import xml.etree.ElementTree as ET
from pathlib import Path

import pytest
import scratch

from plumcraft_engine import new as generator
from plumcraft_engine.cli import main

GOLDEN = Path(__file__).parent / "golden" / "new_engine"
REPO = Path(__file__).resolve().parents[2]
SDK = str(REPO / "python")
ALL = "text,table,array,svg,image,audio,json,list,any"


def gen(dest: Path, *args: str) -> int:
    return main(["new", str(dest), *args])


def tree(root: Path) -> dict[str, str]:
    """Every file under `root`, by relative path, with this checkout's path normalised."""
    skip = {"__pycache__", ".venv", ".pytest_cache", ".ruff_cache"}
    files = [f for f in sorted(root.rglob("*")) if f.is_file() and not skip & set(f.relative_to(root).parts)]
    return {str(f.relative_to(root)): f.read_text("utf-8").replace(SDK, "{{SDK}}") for f in files}


@pytest.fixture
def tmp() -> Path:
    return Path(scratch.mkdtemp("new-"))


@pytest.mark.parametrize(
    "args, message",
    [
        (["--id", "Bad-Id"], "lower case letters"),
        (["--id", "9lives"], "lower case letters"),
        (["--id", "x" * 33], "lower case letters"),
        (["--id", "class"], "Python keyword"),
        (["--id", "text"], "built-in engine"),
        (["--id", "array"], "built-in engine"),
        (["--id", "vectorcraft"], "built-in engine"),
        (["--id", "seq"], "building blocks' prefix"),
        (["--id", "file"], "building blocks' prefix"),
        (["--id", "plumcraft"], "reserved"),
        (["--id", "x"], "reserved"),
        (["--id", "ok", "--package", "not-a-name"], "isn't a Python package name"),
        (["--id", "ok", "--package", "json"], "would shadow"),
        (["--id", "ok", "--package", "plumcraft_engine"], "would shadow"),
        (["--id", "ok", "--types", "nonsense"], "unknown"),
        (["--id", "ok", "--sdk", "pip"], "pypi, path, git or git@REV"),
        (["--id", "ok", "--python", "3.9"], "3.11 or later"),
    ],
)
def test_refusals_write_nothing(tmp, capsys, args, message):
    assert gen(tmp / "new-engine", *args) == 2
    assert message in capsys.readouterr().err
    assert list(tmp.iterdir()) == [], "no project and no temporary directory"


def test_a_directory_with_something_in_it_is_refused(tmp, capsys):
    (tmp / "taken").mkdir()
    (tmp / "taken" / "keep.txt").write_text("mine")
    assert gen(tmp / "taken", "--id", "ok") == 2
    assert "isn't empty" in capsys.readouterr().err
    assert [p.name for p in (tmp / "taken").iterdir()] == ["keep.txt"]
    (tmp / "file").write_text("a file")
    assert gen(tmp / "file", "--id", "ok") == 2


def test_an_empty_directory_is_fine_and_the_id_comes_from_it(tmp):
    (tmp / "river-gauge-engine").mkdir()
    assert gen(tmp / "river-gauge-engine") == 0
    p = tmp / "river-gauge-engine"
    assert (p / "src" / "river_gauge_engine" / "__init__.py").read_text().count("class RiverGaugeEngine(Engine):") == 1
    assert tomllib.loads((p / "pyproject.toml").read_text())["project"]["name"] == "river-gauge-engine"


def test_a_failure_leaves_nothing(tmp, monkeypatch):
    def boom(*_):
        raise generator.Refused("the engine wouldn't load")

    monkeypatch.setattr(generator, "catalogue", boom)
    assert gen(tmp / "new-engine", "--id", "ok") == 2
    assert list(tmp.iterdir()) == []


def test_the_builtin_ids_match_the_packs():
    packs = sorted(p.stem for p in (REPO / "crates" / "craft" / "packs").glob("*.toml"))
    assert list(generator.CRAFTS) == packs


@pytest.mark.parametrize("case, types", [("scalars", "scalars"), ("text", "text"), ("all", ALL)])
def test_the_output_is_the_golden(tmp, case, types):
    dest = tmp / "weather-engine"
    assert gen(dest, "--name", "Weather", "--types", types) == 0
    got = tree(dest)
    root = GOLDEN / case
    if os.environ.get("UPDATE_GOLDEN"):
        for rel, text in got.items():
            (root / rel).parent.mkdir(parents=True, exist_ok=True)
            (root / rel).write_text(text, "utf-8")
    assert got == tree(root)


def in_project(dest: Path, *cmd: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, "-m", *cmd], cwd=dest, capture_output=True, text=True, check=False)


def lint(dest: Path) -> None:
    for cmd in (("ruff", "check", "."), ("ruff", "format", "--check", ".")):
        out = in_project(dest, *cmd)
        assert out.returncode == 0, out.stdout + out.stderr


@pytest.mark.parametrize("types", ["scalars", "text", ALL])
def test_the_output_is_well_formed_and_its_own_tests_pass(tmp, types):
    dest = tmp / "weather-engine"
    assert gen(dest, "--types", types) == 0
    for f in dest.rglob("*.py"):
        compile(f.read_text("utf-8"), str(f), "exec")
    pyproject = tomllib.loads((dest / "pyproject.toml").read_text())
    assert pyproject["tool"]["uv"]["sources"]["plumcraft-engine"] == {"path": SDK, "editable": True}
    manifest = tomllib.loads((dest / "plumcraft-engine.toml").read_text())
    assert manifest["id"] == "weather" and manifest["mode"] == "process"
    assert manifest["demo"] == "examples/demo.json" and manifest["conformance"]["slow"] == "weather.slow"
    lint(dest)
    out = in_project(dest, "pytest", "-q", "-p", "no:cacheprovider")
    assert out.returncode == 0, out.stdout[-3000:] + out.stderr[-1000:]


ALONE = ["table", "array", "svg", "image", "audio", "json", "list", "any", "table,text", "table,array", "svg,text", "image,array", "audio,array", "json,text"]


@pytest.mark.parametrize("types", ALONE)
def test_each_type_and_pair_works_alone(tmp, types):
    dest = tmp / "alone-engine"
    assert gen(dest, "--types", types) == 0
    lint(dest)
    out = in_project(dest, "pytest", "-q", "-p", "no:cacheprovider", "--deselect", "tests/test_conformance.py")
    assert out.returncode == 0, out.stdout[-3000:] + out.stderr[-1000:]


def methods(dest: Path) -> set[str]:
    return set(re.findall(r"def (\w+)\(", next((dest / "src").glob("*/__init__.py")).read_text()))


def test_every_conversion_has_its_inverse(tmp):
    dest = tmp / "weather-engine"
    assert gen(dest, "--types", ALL) == 0
    found = methods(dest)
    assert {"format_number", "parse_number", "text_to_hex", "hex_to_text", "table_to_csv", "csv_to_table", "array_to_audio", "audio_to_array"} <= found
    assert {"Split", "JoinList"} <= set(re.findall(r"^class (\w+)\(Node\)", next((dest / "src").glob("*/__init__.py")).read_text(), re.M))
    for m in found:
        a, sep, b = m.partition("_to_")
        if sep:
            assert f"{b}_to_{a}" in found, m


def test_a_pair_needs_both_its_types(tmp):
    dest = tmp / "one-engine"
    assert gen(dest, "--types", "table") == 0
    assert not {"table_to_csv", "table_to_array"} & methods(dest)


def test_list_brings_text(tmp):
    dest = tmp / "words-engine"
    assert gen(dest, "--types", "list") == 0
    assert "text_to_hex" in methods(dest)


def test_image_depends_on_pillow(tmp):
    assert gen(tmp / "a-engine", "--types", "image") == 0
    assert gen(tmp / "b-engine", "--types", "text") == 0
    deps = lambda n: tomllib.loads((tmp / n / "pyproject.toml").read_text())["project"]["dependencies"]  # noqa: E731
    assert deps("a-engine") == ["plumcraft-engine>=0.1,<0.2", "pillow>=10"]
    assert deps("b-engine") == ["plumcraft-engine>=0.1,<0.2"]


@pytest.mark.parametrize("types", ["text", ALL])
def test_the_demo_uses_every_node(tmp, types):
    dest = tmp / "weather-engine"
    assert gen(dest, "--types", types) == 0
    demo = json.loads((dest / "examples" / "demo.json").read_text())
    golden = json.loads((dest / "tests" / "golden" / "schema.json").read_text())
    own = {n["id"] for n in golden["nodes"] if n["id"].startswith("weather.")}
    assert {v["kind"] for v in demo["nodes"].values()} == own


def test_the_scripts_are_executable_and_parse(tmp):
    dest = tmp / "weather-engine"
    assert gen(dest) == 0
    names = {f.name for f in (dest / "scripts").iterdir()}
    assert names == {"check", "nodes", "run", "serve", "conform", "cli-run", "editor", "install", "uninstall", "_plumcraft.sh"}
    for f in (dest / "scripts").iterdir():
        assert subprocess.run(["bash", "-n", str(f)], check=False).returncode == 0, f.name
        assert os.access(f, os.X_OK) == (f.name != "_plumcraft.sh"), f.name
    assert "python -m weather_engine --listen" in (dest / "scripts" / "serve").read_text()
    assert "engines/weather.toml" in (dest / "scripts" / "install").read_text()


def test_the_pycharm_files_are_well_formed(tmp):
    dest = tmp / "weather-engine"
    assert gen(dest) == 0
    runs = {ET.parse(f).getroot().find("configuration").get("name"): ET.parse(f).getroot().find("configuration") for f in (dest / ".run").glob("*.run.xml")}
    editor = runs.pop("Editor")
    assert editor.get("type") == "ShConfigurationType" and editor.find("option[@name='SCRIPT_PATH']").get("value") == "$PROJECT_DIR$/scripts/editor"
    assert set(runs) == {"Serve (HTTP 7311)", "Run demo", "Run demo (attached)", "Nodes", "Tests", "Conformance"}
    option = lambda c, n: c.find(f"option[@name='{n}']").get("value")  # noqa: E731
    for c in runs.values():
        assert c.find("module").get("name") == "weather-engine"
        assert option(c, "SDK_HOME") == "$PROJECT_DIR$/.venv/bin/python" and option(c, "WORKING_DIRECTORY") == "$PROJECT_DIR$"
    serve = runs["Serve (HTTP 7311)"]
    listen = "--listen 127.0.0.1:7311 --token-file .engine-token"
    assert (option(serve, "SCRIPT_NAME"), option(serve, "PARAMETERS"), option(serve, "MODULE_MODE")) == ("weather_engine", listen, "true")
    assert runs["Tests"].get("type") == "tests" and option(runs["Conformance"], "_new_target") == '"$PROJECT_DIR$/tests/test_conformance.py"'
    module = ET.parse(dest / ".idea" / "weather-engine.iml").getroot()
    folders = {f.get("url"): f.get("isTestSource") for f in module.iter("sourceFolder")}
    assert folders == {"file://$MODULE_DIR$/src": "false", "file://$MODULE_DIR$/tests": "true"}
    assert "weather-engine.iml" in ET.tostring(ET.parse(dest / ".idea" / "modules.xml").getroot(), encoding="unicode")
    assert not (dest / ".idea" / "misc.xml").exists(), "it names a machine's interpreter"
    assert "misc.xml" in (dest / ".gitignore").read_text() and "misc.xml" in (dest / ".idea" / ".gitignore").read_text()


def test_no_pycharm_leaves_them_out(tmp):
    dest = tmp / "weather-engine"
    assert gen(dest, "--no-pycharm") == 0
    assert not (dest / ".run").exists() and not (dest / ".idea").exists()


def test_sdk_pypi_has_no_local_source(tmp):
    dest = tmp / "a-engine"
    assert gen(dest, "--sdk", "pypi") == 0
    pyproject = tomllib.loads((dest / "pyproject.toml").read_text())
    assert pyproject["project"]["dependencies"] == ["plumcraft-engine>=0.1,<0.2"]
    assert "sources" not in pyproject["tool"].get("uv", {})


def test_sdk_git_pins_a_commit(tmp):
    dest = tmp / "pinned-engine"
    assert gen(dest, "--sdk", "git@abc123") == 0
    source = tomllib.loads((dest / "pyproject.toml").read_text())["tool"]["uv"]["sources"]["plumcraft-engine"]
    assert source == {"git": generator.GIT_URL, "subdirectory": "python", "rev": "abc123"}


def test_git_makes_a_first_commit(tmp, monkeypatch):
    dest = tmp / "kept-engine"
    for k, v in {"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com"}.items():
        monkeypatch.setenv(k, v)
    assert gen(dest, "--git") == 0
    log = subprocess.run(["git", "log", "--oneline"], cwd=dest, capture_output=True, text=True, check=True)
    assert "plumcraft-engine new" in log.stdout
