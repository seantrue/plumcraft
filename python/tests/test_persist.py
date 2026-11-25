"""Persisted memo records (spec 14.5), as `crates/engine/tests/persist.rs`: a slow node's result
survives the engine (a fresh memo, the same store) as `cached`; a fast one, a `cache: never` one,
one too big to be worth writing and one whose time was setup are not persisted; `cache: always`
is; a reaped value or a hostile record is a miss that recomputes, never an error.

Whether a result is too big is weighed against the node's measured work (spec 14.5), so a test of
it can't use the wall clock: on a host whose memory is fragmented, the kernel's compaction when
numpy faults in a fresh 8 MB array (numpy asks for huge pages) has taken over 150 ms (B49). Those
tests give the engine a `VirtualClock` (`Engine.with_clock`), on which a node's work is exactly the
time it sleeps on that clock."""

import json
import os

import numpy as np
import pytest
import scratch
from commonkit import digest as ck_digest

from plumcraft_engine import Blob, CancelToken, Clock, Engine, List, Node, RealClock, VirtualClock, data, exec


class Slow(Node):
    """Sleeps `ms` on `clock` (of which `setup` counts as setup), then makes one of every kind of
    value."""

    def __init__(self, ident="t.slow", scope="machine", clock: Clock | None = None):
        ints = [{"name": n, "type": "INT", "default": d} for n, d in (("n", 1), ("ms", 0), ("setup", 0), ("big", 0))]
        outs = [{"name": o, "type": "ANYTHING"} for o in ("json", "array", "blob", "list")]
        self.definition = {"id": ident, "inputs": ints, "outputs": outs, "memo": {"scope": scope}}
        self.runs = 0
        self.clock = clock or RealClock()

    def run(self, inputs, cx):
        self.runs += 1
        self.clock.sleep(inputs["ms"] / 1000)
        cx.setup_took(inputs["setup"] / 1000)
        n = inputs["n"]
        text = (json.dumps({"n": n}) + " ") * 20_000
        meta = {"media": "application/json", "size": len(text), "form": {"kind": "doc", "app": "t"}}
        return {
            "json": {"n": n, "list": [1, 2.5, "three"]},
            "array": np.full(inputs["big"], n % 251, dtype=np.uint8),
            "blob": Blob(handle=data.raw(text.encode()), meta=meta, data=text.encode()),
            "list": List([n, n + 1], [("t.axis", 2)], ["a", "b"]),
        }


#: A digest of this process's length that names nothing, and an nd1 handle of it.
ZERO = "0" * len(ck_digest.raw(b""))
ZERO_ND1 = f"{ck_digest.current()}:nd1:{ZERO}"


def root() -> str:
    return scratch.mkdtemp("persist-")


def engine(store_root, salt="s", clock: Clock | None = None):
    """An engine whose nodes sleep, and have their work measured, on `clock` (the real one when
    none is given)."""
    clock = clock or RealClock()
    slow, session = Slow(clock=clock), Slow("t.session", "session", clock)
    e = Engine({"id": "t", "salt": salt}).with_store(store_root).with_persist_ms(40.0).with_wire_type({"id": "ANYTHING", "media": "tree"})
    return e.with_clock(clock).with_node(slow).with_node(session), slow, session


def run(e, store_root, node, memo="use"):
    graph = {"kind": "inline", "graph": {"version": 1, "nodes": {"n": node}, "wires": []}}
    req = {"run_id": "r", "generation": 1, "memo": memo, "targets": ["n"], "graph": graph}
    return exec.run(e, req, lambda _: None, CancelToken(), "test", exec.Memo(), data.Plane(store_root, True, 1 << 20))


def twice(store_root, node):
    """Run `node` in one engine, then in a fresh one over the same store, both on a virtual clock:
    its state there, and how many times it ran in all."""
    clock = VirtualClock()
    a, sa, ta = engine(store_root, clock=clock)
    assert run(a, store_root, node)["nodes"]["n"]["state"] == "done"
    b, sb, tb = engine(store_root, clock=clock)
    state = run(b, store_root, node)["nodes"]["n"]["state"]
    return state, sa.runs + ta.runs + sb.runs + tb.runs


def test_a_slow_result_survives_a_restart_whole():
    r = root()
    node = {"kind": "t.slow", "values": {"n": 7, "ms": 60, "big": 1000}}
    a, _, _ = engine(r)
    first = run(a, r, node)
    b, slow, _ = engine(r)
    again = run(b, r, node)
    assert again["nodes"]["n"]["state"] == "cached" and slow.runs == 0
    assert again["nodes"]["n"]["outputs"] == first["nodes"]["n"]["outputs"], "the same values, by handle"
    is_ = again["nodes"]["n"]["memo"]
    assert is_["source"] == "store" and is_["computed_ms"] >= 60.0
    assert first["stats"]["computed"] == 1 and first["stats"]["kept"] == 1
    read = again["stats"]
    assert read["computed"] == 0 and read["from_store"] == 1 and read["saved_ms"] >= 60.0
    assert read["store"].get("shm_hit", 0) > 0, "commonkit's counters come too"
    assert read["tree"]["free_bytes"] > 0 and read["tree"]["min_free_bytes"] > 0, "the disk, even with no walk yet"
    c, slow, _ = engine(r, salt="new code")
    assert run(c, r, node)["nodes"]["n"]["state"] == "done" and slow.runs == 1, "new code: a new key"
    d, slow, _ = engine(r)
    assert run(d, r, node, "refresh")["nodes"]["n"]["state"] == "done"
    assert run(d, r, node, "off")["nodes"]["n"]["state"] == "done"
    assert slow.runs == 2


def test_fast_never_session_too_big_and_setup_bound_results_are_not_persisted():
    r = root()
    assert twice(r, {"kind": "t.slow", "values": {"n": 1}}) == ("done", 2), "fast"
    assert twice(r, {"kind": "t.slow", "values": {"n": 2, "ms": 60}, "cache": "never"}) == ("done", 2), "never"
    assert twice(r, {"kind": "t.session", "values": {"n": 3, "ms": 60}}) == ("done", 2), "session scope"
    assert twice(r, {"kind": "t.slow", "values": {"n": 4, "ms": 60, "big": 8_000_000}}) == ("done", 2), "too big"
    assert twice(r, {"kind": "t.slow", "values": {"n": 5, "ms": 60, "setup": 60}}) == ("done", 2), "setup"
    assert twice(r, {"kind": "t.slow", "values": {"n": 6}, "cache": "always"}) == ("cached", 1), "always"
    assert twice(r, {"kind": "t.slow", "values": {"n": 8}, "cache": ["junk"]}) == ("done", 2), "junk is auto"


def test_too_big_is_weighed_against_the_work_measured():
    """B49: about 8.2 MB of outputs costs ~82 ms to write and read back at the rule's 200 MB/s, so
    60 ms of work isn't worth persisting, and the 96.8 ms a run measured on a busy host is."""
    r = root()
    assert twice(r, {"kind": "t.slow", "values": {"n": 10, "ms": 60, "big": 8_000_000}}) == ("done", 2), "60 ms: too big"
    a, _, _ = engine(r, clock=VirtualClock())
    busy = run(a, r, {"kind": "t.slow", "values": {"n": 11, "ms": 97, "big": 8_000_000}})["nodes"]["n"]
    assert busy["memo"]["computed_ms"] == pytest.approx(97.0), "the work is what the node slept"
    assert twice(r, {"kind": "t.slow", "values": {"n": 12, "ms": 97, "big": 8_000_000}}) == ("cached", 1), "97 ms: worth it"


def test_work_is_measured_on_the_engines_clock():
    """The engine's clock, not the machine's: a node that sleeps no time on a virtual clock did no
    work, however long it really took; one that sleeps on it did exactly that much; and an engine
    measures on the real clock unless it is given another."""
    assert isinstance(Engine({"id": "t", "salt": "s"}).clock, RealClock)
    r = root()

    class Busy(Node):
        definition = {"id": "t.busy", "inputs": [], "outputs": [{"name": "n", "type": "ANYTHING"}], "memo": {"scope": "machine"}}

        def run(self, inputs, cx):
            sum(range(200_000))  # real work the virtual clock doesn't see
            return {"n": 1}

    clock = VirtualClock()
    e, _, _ = engine(r, clock=clock)
    e.with_node(Busy())
    assert run(e, r, {"kind": "t.busy", "values": {}})["nodes"]["n"]["memo"]["computed_ms"] == 0.0
    assert run(e, r, {"kind": "t.slow", "values": {"n": 13, "ms": 250}})["nodes"]["n"]["memo"]["computed_ms"] == pytest.approx(250.0)
    assert clock.now() == pytest.approx(0.25), "the node slept on it, no real time"


def test_a_reaped_value_or_a_hostile_record_is_a_miss_that_recomputes():
    r = root()
    node = {"kind": "t.slow", "values": {"n": 9, "ms": 60}}
    a, _, _ = engine(r)
    first = run(a, r, node)
    key = first["nodes"]["n"]["memo"]["plan_key"]
    assert data.read_record(r, key) is not None, "persisted"
    blob = first["nodes"]["n"]["outputs"]["blob"]["ref"]["handle"]
    store = data.open_store(r)
    for k in (data.key_for(blob), data.z_key_for(blob)):
        if os.path.isfile(store.path_for(k)):
            os.unlink(store.path_for(k))
    b, slow, _ = engine(r)
    assert run(b, r, node)["nodes"]["n"]["state"] == "done" and slow.runs == 1, "reaped: recomputed"
    c, _, _ = engine(r)
    assert run(c, r, node)["nodes"]["n"]["state"] == "cached", "and persisted again"
    nest = {"json": 1}
    for _ in range(40):
        nest = {"list": {"items": [nest]}}
    for junk in [
        b"not json",
        json.dumps({"v": 1, "plan_key": f"memo:{ck_digest.current()}:" + ZERO, "outputs": {}}).encode(),
        json.dumps({"v": 2, "plan_key": key, "outputs": {}}).encode(),
        json.dumps({"v": 1, "plan_key": key, "outputs": {"json": {"what": 1}}}).encode(),
        json.dumps({"v": 1, "plan_key": key, "outputs": {"json": {"json": "only one of four outputs"}}}).encode(),
        json.dumps({"v": 1, "plan_key": key, "outputs": {"array": {"array": {"handle": ZERO_ND1, "dtype": "|u1", "shape": [1]}}}}).encode(),
        json.dumps({"v": 1, "plan_key": key, "outputs": {"array": {"array": "not an object"}}}).encode(),
        json.dumps({"v": 1, "plan_key": key, "outputs": {"l": nest}}).encode(),
    ]:
        assert data.put_record(r, key, junk)
        d, slow, _ = engine(r)
        assert (run(d, r, node)["nodes"]["n"]["state"], slow.runs) == ("done", 1), junk
