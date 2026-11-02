"""`plumcraft_engine.conformance`: the text engine passes every case in process and over HTTP, and
each deliberately broken engine fails, the case named. `plumcraft-engine check` reports the same."""

import json
import time
from pathlib import Path
from typing import Annotated

from plumcraft_engine import ConfigError, Cx, Engine, File, NodeError, OpError, Range, String, Text, WireType, node
from plumcraft_engine.cli import main
from plumcraft_engine.conformance import assert_conformant, check, core_declarations
from plumcraft_text import TextEngine

DEMO = {
    "nodes": {"a": {"kind": "text.literal", "values": {"text": "the corgi"}}, "b": {"kind": "text.upper"}},
    "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "b", "port": "text"}}],
}


def failed(results: list) -> dict[str, str]:
    return {r.case: r.why for r in results if not r.ok}


class Base(Engine):
    id = "b"
    name = "Base"
    version = "1"

    @node
    def word(self, w: str = "corgi") -> Text:
        return w

    @node
    def slow(self, text: Text, seconds: Annotated[float, Range(0, 60, invalid="clamp")] = 0.1, *, cx: Cx) -> Text:
        t = time.monotonic()
        while time.monotonic() - t < seconds:
            if cx.cancelled():
                raise NodeError.cancelled()
            time.sleep(0.01)
        return text


def test_the_text_engine_is_conformant_in_process():
    assert_conformant(TextEngine, demo=DEMO, slow="text.slow", modes=("process",))


def test_a_healthy_small_engine_is_conformant():
    assert_conformant(Base, modes=("process",))


def test_what_isnt_given_is_skipped_not_passed_silently():
    class NoSlow(Engine):
        id = "n"
        name = "NoSlow"
        version = "1"

        @node
        def word(self, w: str = "x") -> Text:
            return w

    results = check(NoSlow, modes=("process",))
    assert not failed(results)
    why = {r.case: r.why for r in results}
    assert why["process: the demo"] == "skipped: no demo given"
    assert why["process: cancel mid-run"] == "skipped: no slow node"


def test_a_node_that_sleeps_through_cancel_fails_cancel():
    class Stubborn(Base):
        id = "s"

        @node
        def slow(self, text: Text, seconds: Annotated[float, Range(0, 60, invalid="clamp")] = 0.1) -> Text:
            time.sleep(min(seconds, 1.3))  # never looks at cx: 1 s past the cancel, well over the limit
            return text

    bad = failed(check(Stubborn, modes=("process",), slow="s.slow"))
    assert "process: cancel mid-run" in bad
    assert "s.slow" in bad["process: cancel mid-run"]


class Stray(Base):
    id = "t"

    @node(id="other.word")
    def stray(self, w: str = "x") -> String:
        return w


def test_a_source_that_needs_a_file_isnt_used_as_a_source():
    class FileFirst(Engine):
        id = "a_file"  # sorted first, so a naive search finds `load` before `word`
        name = "FileFirst"
        version = "1"

        @node
        def load(self, path: Annotated[str, File("open")] = "") -> Text:
            raise ConfigError("choose a file")

        @node
        def word(self, w: str = "corgi") -> Text:
            return w

        @node
        def slow(self, text: Text, seconds: Annotated[float, Range(0, 60, invalid="clamp")] = 0.05, *, cx: Cx) -> Text:
            t = time.monotonic()
            while time.monotonic() - t < seconds:
                if cx.cancelled():
                    raise NodeError.cancelled()
                cx.progress(0.5, "waiting")
                time.sleep(0.01)
            return text

    assert not failed(check(FileFirst, modes=("process",)))


def test_a_node_id_without_the_prefix_fails_session():
    bad = failed(check(Stray, modes=("process",)))
    assert "other.word" in bad["process: session"]


def test_a_core_type_declared_its_own_way_fails_core_types():
    Wrong = Annotated[str, WireType("TEXT", media="text", color="#000000")]

    class OwnText(Engine):
        id = "o"
        name = "OwnText"
        version = "1"

        @node
        def word(self, w: str = "x") -> Wrong:
            return w

    bad = failed(check(OwnText, modes=("process",)))
    assert "TEXT" in bad["process: core types"]


def test_a_demo_that_fails_names_the_node():
    class Failing(Base):
        id = "f"

        @node
        def broken(self, text: Text) -> Text:
            raise OpError("no")

    wires = [{"from": {"node": "w", "port": "text"}, "to": {"node": "x", "port": "text"}}]
    demo = {"nodes": {"w": {"kind": "b.word"}, "x": {"kind": "f.broken"}}, "wires": wires}  # inherited nodes keep their prefix
    bad = failed(check(Failing, demo=demo, modes=("process",)))
    assert "'x': 'error'" in bad["process: the demo"]
    refused = {"nodes": {"w": {"kind": "f.nothing"}}, "wires": []}
    assert "the run was refused" in failed(check(Failing, demo=refused, modes=("process",)))["process: the demo"]


def test_an_engine_that_wont_start_is_one_failure():
    def broken():
        raise RuntimeError("no engine today")

    results = check(broken, modes=("process",))
    assert len(results) == 1 and not results[0].ok and "no engine today" in results[0].why


def test_core_declarations_name_all_six():
    assert set(core_declarations()) == {"ARRAY", "SVG", "TEXT", "TABLE", "IMAGE", "AUDIO", "JSON"}


def test_check_exits_by_its_cases(tmp_path, capsys, monkeypatch):
    p = Path(tmp_path)
    (p / "demo.json").write_text(json.dumps(DEMO))
    table = '[tool.plumcraft]\nengine = "plumcraft_text:TextEngine"\ndemo = "demo.json"\n'
    (p / "pyproject.toml").write_text(table + '\n[tool.plumcraft.conformance]\nslow = "text.slow"\n')
    monkeypatch.chdir(p)
    assert main(["check"]) == 0
    out = capsys.readouterr().out
    assert "14 of 14 cases hold" in out and "ok    http: the demo" in out
    assert main(["check", "--engine", "test_conformance:Stray", "--mode", "process"]) == 1, "a broken engine"
    assert "FAIL  process: session" in capsys.readouterr().out
    assert main(["check", "--mode", "nope"]) == 2


def test_check_without_an_engine_exits_2(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    assert main(["check"]) == 2
    assert main(["check", "--engine", "nope:X"]) == 2
