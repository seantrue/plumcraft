"""`graph_info.hash` from its specification alone (docs/interface.md section 15; decision 1B): a
second implementation, from the workflow JSON, of the result form and its digest, checked against
the shared vectors the Rust session's `trust::result_hash` makes
(`crates/proto/fixtures/result_hash.json`)."""

import hashlib
import json
import pathlib

from plumcraft_engine import memo

FIXTURE = pathlib.Path(__file__).parents[2] / "crates" / "proto" / "fixtures" / "result_hash.json"


def result_form(graph: dict) -> dict:
    nodes = {}
    for nid, n in (graph.get("nodes") or {}).items():
        o = {"kind": n.get("kind", ""), "values": dict(n.get("values") or {})}
        if n.get("engine") is not None:
            o["engine"] = n["engine"]
        if "convert" in n:
            o["convert"] = n["convert"]
        nodes[nid] = o
    wires = []
    for w in graph.get("wires") or []:
        o = {"from": {"node": w["from"]["node"], "port": w["from"]["port"]}, "to": {"node": w["to"]["node"], "port": w["to"]["port"]}}
        if w.get("slice"):
            o["slice"] = w["slice"]
        wires.append(o)
    key = lambda w: tuple(s.encode("utf-8") for s in (w["to"]["node"], w["to"]["port"], w["from"]["node"], w["from"]["port"]))  # noqa: E731
    wires.sort(key=key)
    return {"format": "plumcraft.result", "v": 1, "nodes": nodes, "wires": wires}


def result_hash(graph: dict) -> str:
    return "sha256:" + hashlib.sha256(memo.canon(result_form(graph)).encode("utf-8")).hexdigest()


def test_the_vectors_from_the_specification_alone():
    fx = json.loads(FIXTURE.read_text("utf-8"))
    g = fx["graphs"]
    assert memo.canon(result_form(g["base"])) == fx["expect"]["base"]["canon"]
    for name in ("base", "moved", "param", "wire"):
        assert result_hash(g[name]) == fx["expect"][name]["hash"], name
    assert result_hash(g["moved"]) == result_hash(g["base"]) != result_hash(g["param"]) != result_hash(g["wire"])
