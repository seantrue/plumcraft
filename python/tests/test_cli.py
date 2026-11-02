"""`plumcraft-engine` (plumcraft_engine.cli), driving the text engine in process and over HTTP: the
catalogue, a run's events and values, the exit code following node states, `[tool.plumcraft]`
lookup, relative paths anchored at the workflow, cancel, `--raw`, and every mistake exit 2."""

import json
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

import pytest
import scratch

from plumcraft_engine.cli import main

TEXT = "plumcraft_text:TextEngine"


def workflow(path: Path, nodes: dict, wires: list | None = None) -> str:
    path.write_text(json.dumps({"nodes": nodes, "wires": wires or []}))
    return str(path)


def wire(a: str, b: str) -> dict:
    return {"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}}


@pytest.fixture
def tmp() -> Path:
    return Path(scratch.mkdtemp("cli-"))


def test_nodes_lists_the_catalogue(capsys):
    assert main(["nodes", "--engine", TEXT]) == 0
    out = capsys.readouterr().out
    assert any(line.startswith("text.upper ") and "(text:TEXT) -> (text:TEXT)" in line for line in out.splitlines()), out


def test_a_run_prints_events_and_values_and_exits_0(tmp, capsys):
    w = workflow(tmp / "w.json", {"a": {"kind": "text.literal", "values": {"text": "the corgi"}}, "b": {"kind": "text.upper"}}, [wire("a", "b")])
    assert main(["run", w, "--engine", TEXT, "--watch", "b.text"]) == 0
    out = capsys.readouterr().out
    assert out.startswith("(Text "), out
    assert "b          done" in out and "watched b.text (text)" in out and "  | THE CORGI" in out
    assert out.rstrip().splitlines()[-1].startswith("run cli-") and "2 done" in out


def test_a_failed_node_exits_1(tmp, capsys):
    nodes = {"a": {"kind": "text.literal", "values": {"text": "x"}}, "f": {"kind": "text.fail"}, "after": {"kind": "text.upper"}}
    w = workflow(tmp / "w.json", nodes, [wire("a", "f"), wire("f", "after")])
    assert main(["run", w, "--engine", TEXT]) == 1
    out = capsys.readouterr().out
    assert "op: this node always fails" in out and "skipped" in out


def test_mistakes_exit_2(tmp, capsys, monkeypatch):
    monkeypatch.chdir(tmp)
    assert main([]) == 2
    assert main(["run", "--engine"]) == 2
    assert main(["nodes"]) == 2, "no engine and no [tool.plumcraft]"
    assert main(["nodes", "--engine", "nope_module:X"]) == 2
    assert main(["nodes", "--engine", "plumcraft_text"]) == 2
    assert main(["run", str(tmp / "missing.json"), "--engine", TEXT]) == 2
    assert main(["run", "--engine", TEXT, "--attach", "http://127.0.0.1:1"]) == 2, "one or the other"
    assert main(["nodes", "--attach", "http://127.0.0.1:1"]) == 2, "nothing listening"
    assert main(["run", "--engine", TEXT, "--watch", "nodot"]) == 2
    (tmp / "bad.json").write_text("[1, 2]")
    assert main(["run", str(tmp / "bad.json"), "--engine", TEXT]) == 2
    err = capsys.readouterr().err
    assert "no engine: give --engine" in err and "nope_module" in err and "not a workflow" in err


def test_the_project_names_the_engine_and_demo(tmp, capsys, monkeypatch):
    (tmp / "pyproject.toml").write_text(f'[project]\nname = "x"\n\n[tool.plumcraft]\nengine = "{TEXT}"\ndemo = "examples/demo.json"\n')
    (tmp / "examples").mkdir()
    workflow(tmp / "examples" / "demo.json", {"a": {"kind": "text.literal", "values": {"text": "hi"}}})
    (tmp / "deeper").mkdir()
    monkeypatch.chdir(tmp / "deeper")
    assert main(["run"]) == 0, "found from a folder below"
    assert "1 done" in capsys.readouterr().out


def test_relative_paths_are_the_workflows(tmp, capsys, monkeypatch):
    (tmp / "words.txt").write_text("from beside the workflow")
    w = workflow(tmp / "w.json", {"l": {"kind": "text.load", "values": {"path": "words.txt"}}})
    monkeypatch.chdir(Path(scratch.mkdtemp("cli-elsewhere-")))
    assert main(["run", w, "--engine", TEXT, "--watch", "l.text"]) == 0
    assert "  | from beside the workflow" in capsys.readouterr().out


def test_cancel_after_ends_the_run_cancelled(tmp, capsys):
    nodes = {"a": {"kind": "text.literal", "values": {"text": "x"}}, "s": {"kind": "text.slow", "values": {"seconds": 5}}}
    w = workflow(tmp / "w.json", nodes, [wire("a", "s")])
    t0 = time.monotonic()
    assert main(["run", w, "--engine", TEXT, "--cancel-after", "0.3"]) == 1
    assert time.monotonic() - t0 < 4
    assert "cancel     sent" in capsys.readouterr().out


def test_raw_prints_every_event_as_json(tmp, capsys):
    w = workflow(tmp / "w.json", {"a": {"kind": "text.literal", "values": {"text": "x"}}})
    assert main(["run", w, "--engine", TEXT, "--raw"]) == 0
    events = [json.loads(line) for line in capsys.readouterr().out.splitlines()]
    assert events[-1]["kind"] == "run.reply" and any(e["kind"] == "node.status" for e in events)


def test_attach_runs_the_same_over_http(tmp, capsys, monkeypatch):
    from plumcraft_engine.http import serve_http
    from plumcraft_text import TextEngine

    port_file = tmp / "port"
    threading.Thread(target=serve_http, args=(TextEngine, "127.0.0.1:0", "t0ken", str(port_file)), daemon=True).start()
    deadline = time.monotonic() + 10
    while not (port_file.exists() and port_file.read_text().strip()) and time.monotonic() < deadline:
        time.sleep(0.02)
    url = f"http://{port_file.read_text().strip()}"
    monkeypatch.delenv("PLUMCRAFT_ENGINE_TOKEN", raising=False)
    monkeypatch.chdir(tmp)
    assert main(["nodes", "--attach", url]) == 2, "no token, no answer"
    with pytest.raises(urllib.error.HTTPError) as refused:
        urllib.request.urlopen(f"{url}/v1/health", timeout=5)
    assert refused.value.code == 401, "health too"
    (tmp / ".engine-token").write_text("t0ken\n")  # what scripts/serve leaves in a project
    assert main(["nodes", "--attach", url]) == 0
    w = workflow(tmp / "w.json", {"a": {"kind": "text.literal", "values": {"text": "far away"}}, "b": {"kind": "text.upper"}}, [wire("a", "b")])
    assert main(["run", w, "--attach", url, "--watch", "b.text"]) == 0
    out = capsys.readouterr().out
    assert f"at {url}" in out and "  | FAR AWAY" in out
