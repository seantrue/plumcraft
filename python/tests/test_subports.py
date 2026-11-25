"""A choice's sub-ports reach the node (spec 7.6 `options`, 2026-11-22): the chosen option's ports
arrive as `<port>.<sub>`, from the node's value, a wire or the default, and key the memo; another
option's don't. The Rust SDK's executor had the same gap (the ports test engine found it)."""

import pytest

from plumcraft_engine import NodeError
from plumcraft_engine import exec as ex

JOIN = {
    "id": "t.join",
    "inputs": [
        {
            "name": "how",
            "type": "ENUM",
            "default": "line",
            "choices": ["line", "space"],
            "options": {"line": [{"name": "width", "type": "INT", "default": 80}]},
        },
    ],
}


def resolve(values, upstream=(), outputs=None):
    return ex._resolve_inputs(JOIN, {"values": values}, list(upstream), outputs or {})


def test_the_chosen_options_sub_port_arrives_from_its_value_or_default():
    inputs, params = resolve({"how": "line", "how.width": 3.0})
    assert inputs["how.width"] == 3 and isinstance(inputs["how.width"], int), "coerced to INT"
    assert params["how.width"] == 3, "it keys the memo"
    inputs, _ = resolve({})
    assert inputs["how"] == "line" and inputs["how.width"] == 80, "the defaults"


def test_another_options_sub_port_does_not_arrive():
    inputs, params = resolve({"how": "space", "how.width": 3})
    assert "how.width" not in inputs and "how.width" not in params


def test_a_wired_sub_port_takes_the_wire():
    wire = {"from": {"node": "n0", "port": "w"}, "to": {"node": "n1", "port": "how.width"}}
    inputs, params = resolve({"how": "line"}, [wire], {"n0": {"w": 7}})
    assert inputs["how.width"] == 7 and "how.width" not in params


def test_a_required_sub_port_with_nothing_is_refused():
    d = {"id": "t.x", "inputs": [{"name": "how", "type": "ENUM", "default": "a", "options": {"a": [{"name": "n", "type": "INT"}]}}]}
    with pytest.raises(NodeError):
        ex._resolve_inputs(d, {"values": {}}, [], {})


JOIN_ALL = {
    "id": "t.joinall",
    "inputs": [{"name": "c", "type": "TEXT", "socket": "socket", "variadic": {"template": "c{n}", "min": 2, "max": 4}}],
}


def test_a_variadic_inputs_wired_slots_arrive_by_their_names():
    wires = [
        {"from": {"node": "a", "port": "t"}, "to": {"node": "j", "port": "c1"}},
        {"from": {"node": "b", "port": "t"}, "to": {"node": "j", "port": "c3"}},
        {"from": {"node": "b", "port": "t"}, "to": {"node": "j", "port": "c9"}},
    ]
    inputs, params = ex._resolve_inputs(JOIN_ALL, {"values": {}}, wires, {"a": {"t": "alpha"}, "b": {"t": "beta"}})
    assert inputs == {"c1": "alpha", "c3": "beta"}, "c9 is past max: not a slot"
    assert params == {}
    with pytest.raises(NodeError):
        ex._resolve_inputs(JOIN_ALL, {"values": {}}, [], {})
    assert ex._is_slot({"template": "c{n}", "max": 4}, "c4") and not ex._is_slot({"template": "c{n}", "max": 4}, "c0")
    assert not ex._is_slot({"template": "c{n}"}, "cx") and ex._is_slot({"names": ["left"]}, "left")
