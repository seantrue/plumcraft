"""The shared graph vectors (`conformance/graphs/`, `docs/interface.md` 7.6) on the Python SDK, as
`crates/coordinator/tests/vectors.rs` runs them on the Rust engines: each vector whose kinds the
Python SDK's building blocks have is run by the Python executor and held to `expect` (the SDK lists
`lists.nested`). The vectors it can't run (another engine's kinds) are named here, so one that
becomes runnable, or a new one that isn't, fails until this list says so."""

import json
import pathlib

from plumcraft_engine import CancelToken, Engine, Node, exec

ROOT = pathlib.Path(__file__).resolve().parents[2] / "conformance" / "graphs"

# Kinds the Python SDK's building blocks don't have: the text engine's Text and Upper case, the
# test engine's Items (its Join has a stand-in here).
NOT_HERE = {
    "05-item-and-whole-list-inputs.json": "the test engine's ports.items and ports.mixed",
    "09-one-item-a-run-unchanged.json": "the text engine's text.literal and text.upper",
}


class Join(Node):
    """A stand-in for the test engine's Join (`ports.join`, vector 11): its variadic slots joined."""

    definition = {
        "id": "ports.join",
        "inputs": [
            {"name": "c", "type": "STRING", "variadic": {"template": "c{n}", "names": [], "min": 2, "max": 4}},
            {"name": "how", "type": "STRING", "default": "space"},
        ],
        "outputs": [{"name": "text", "type": "STRING"}],
    }

    def run(self, inputs, cx):
        return {"text": " ".join(str(inputs[f"c{n}"]) for n in range(1, 5) if f"c{n}" in inputs)}


def engine():
    return Engine({"id": "py", "salt": "py"}).with_store(None).with_wire_type({"id": "FLOAT", "media": "scalar"}).with_building_blocks().with_node(Join())


def shape(v):
    if v.get("kind") != "list":
        return []
    items = v.get("items") or []
    inner = [shape(i) for i in items]
    out = [len(items)]
    if inner:
        assert all(s == inner[0] for s in inner), f"ragged: {inner}"
        out += inner[0]
    return out


def plain(v):
    return [plain(i) for i in v["items"]] if v.get("kind") == "list" else v.get("value")


def test_every_vector_the_python_sdk_can_run_holds_on_it():
    e = engine()
    have = {n.definition["id"] for n in e.nodes.values()} if isinstance(e.nodes, dict) else set()
    assert "lists.nested" in e.caps
    files = sorted(ROOT.glob("*.json"))
    assert len(files) >= 12
    ran, skipped = [], []
    for f in files:
        v = json.loads(f.read_text())
        kinds = {n["kind"] for n in v["graph"]["nodes"].values()}
        # one executor: a vector across engines runs here as in one engine, which is what it
        # must give (vector 8)
        if not kinds <= have:
            skipped.append(f.name)
            continue
        targets = sorted({k.split(".", 1)[0] for k in v["expect"]})
        req = {"run_id": f.stem, "generation": 1, "graph": {"kind": "inline", "graph": v["graph"]}, "targets": targets, "memo": "off"}
        r = exec.run(e, req, lambda _: None, CancelToken(), "test", exec.Memo())
        assert r["ok"], f"{f.name}: {r}"
        for at, want in v["expect"].items():
            node, port = at.split(".", 1)
            got = r["nodes"][node]["outputs"][port]
            assert shape(got) == want["shape"], f"{f.name} {at}: {shape(got)} not {want['shape']}"
            if "values" in want:
                assert plain(got) == want["values"], f"{f.name} {at}: {plain(got)}"
        ran.append(f.name)
    assert sorted(skipped) == sorted(NOT_HERE), f"the vectors this SDK can't run changed: {skipped}"
    assert len(ran) >= 9, ran
