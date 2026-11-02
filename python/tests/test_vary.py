"""Lists in the executor (spec 7.6 rule 2) and the Vary nodes, as `crates/engine/tests/vary.rs`
and the unit tests in `crates/engine/src/vary.rs`: a list into an input that takes one value runs
the node once per item; lists zip along a shared axis and form a product over different ones; Vary
Range 2D's outputs zip; Vary Choices gets the choices it feeds; errors, cancel and the memo."""

import json
import math
import os

import pytest

from plumcraft_engine import CancelToken, Engine, List, Node, NodeError, exec, slices
from plumcraft_engine.vary import BOOL, FLOAT, INT, STRING, linspace, parse


class Mul(Node):
    """`a × b`, and `mode` (an ENUM) as text: one value each."""

    definition = {
        "id": "t.mul",
        "inputs": [
            {"name": "a", "type": "FLOAT", "default": 1.0},
            {"name": "b", "type": "FLOAT", "default": 1.0},
            {"name": "mode", "type": "ENUM", "default": "plain", "choices": ["plain", "bold", "loud"]},
        ],
        "outputs": [{"name": "out", "type": "FLOAT"}, {"name": "mode", "type": "STRING"}],
    }

    def run(self, inputs, cx):
        a, b = inputs["a"], inputs["b"]
        if a < 0:
            raise NodeError(NodeError.CONFIG, "a is negative")
        return {"out": float(a * b), "mode": inputs.get("mode")}


class Sum(Node):
    """The sum of a whole list."""

    definition = {
        "id": "t.sum",
        "inputs": [{"name": "xs", "type": "FLOAT", "list": True}],
        "outputs": [{"name": "sum", "type": "FLOAT"}],
    }

    def run(self, inputs, cx):
        xs = inputs["xs"]
        if not isinstance(xs, List):
            raise NodeError(NodeError.CONFIG, "xs is not a list")
        return {"sum": float(sum(xs.items))}


def engine():
    e = Engine({"id": "t", "salt": "t"}).with_store(None).with_wire_type({"id": "FLOAT", "media": "scalar"})
    return e.with_node(Mul()).with_node(Sum()).with_building_blocks()


def go_with(e, memo, nodes, wires, targets, watch=(), cancel=None):
    def wire(a, p, b, q):
        # `port|slice` puts a slice on the wire
        port, _, spec = q.partition("|")
        return {"from": {"node": a, "port": p}, "to": {"node": b, "port": port}, **({"slice": spec} if spec else {})}

    wires = [wire(*w) for w in wires]
    graph = {"kind": "inline", "graph": {"version": 1, "nodes": nodes, "wires": wires}}
    req = {"run_id": "r", "generation": 1, "graph": graph, "targets": targets, "watch": list(watch)}
    log = []
    reply = exec.run(e, req, log.append, cancel or CancelToken(), "test", memo)
    return reply, log


def go(nodes, wires, targets):
    return go_with(engine(), exec.Memo(), nodes, wires, targets)[0]


def out(r, node, port):
    """A target's output as plain JSON (lists as arrays)."""

    def plain(v):
        if v["kind"] == "json":
            return v["value"]
        if v["kind"] == "list":
            return [plain(x) for x in v["items"]]
        return repr(v)

    v = r["nodes"].get(node, {}).get("outputs", {}).get(port)
    return None if v is None else plain(v)


def same(got, want):
    """Equal, with floats as floats (an INT 1 is not a FLOAT 1.0, as in JSON)."""
    assert got == want and [type(x) for x in got] == [type(x) for x in want], (got, want)


# The parser and linspace (the unit tests in vary.rs).


def test_parses_lists_and_ranges_as_the_research_software_does():
    same(parse("0.5, 1, 2", FLOAT), [0.5, 1.0, 2.0])
    same(parse("1:3", INT), [1, 2, 3])
    assert len(parse("0:1:0.25", FLOAT)) == 5
    same(parse("0:1/3", FLOAT), [0.0, 0.5, 1.0])
    same(parse("10:0:-5", INT), [10, 5, 0])
    assert parse("a, , b", STRING) == ["a", "b"]
    assert parse("yes, OFF, 1", BOOL) == [True, False, True]
    bads = [("", FLOAT), ("1.5", INT), ("0:1:0.3", INT), ("1:0", FLOAT), ("0:1:0", FLOAT), ("0:1/0", FLOAT)]
    bads += [("maybe", BOOL), ("x", FLOAT), ("0:1e9", FLOAT), ("nan", FLOAT)]
    for bad, kind in bads:
        with pytest.raises(NodeError) as e:
            parse(bad, kind)
        assert e.value.kind == "config", bad


def test_numbers_parse_as_rust_reads_them():
    # Python's float() takes these; Rust's f64 parser doesn't, so neither does Vary.
    for bad in ["1_000", "١", "0x10", "1:2/٣"]:
        with pytest.raises(NodeError):
            parse(bad, FLOAT)
    same(parse("+1, .5, 5., 1e2", FLOAT), [1.0, 0.5, 5.0, 100.0])
    with pytest.raises(NodeError, match="more than 1000 values"):
        parse("0:1/1001", FLOAT)
    with pytest.raises(NodeError, match="more than 1000 values"):
        parse(", ".join(["1"] * 1001), INT)


def test_linspace_matches_numpy():
    assert linspace(1.0, 3.0, 5, True) == [1.0, 1.5, 2.0, 2.5, 3.0]
    assert linspace(0.0, 1.0, 4, False) == [0.0, 0.25, 0.5, 0.75]
    assert linspace(2.0, 9.0, 1, True) == [2.0]
    assert linspace(0.0, 1.0, 3, True)[-1] == 1.0


# The executor (crates/engine/tests/vary.rs).


def test_a_vary_list_runs_a_node_once_per_value():
    r = go({"v": {"kind": "vary.float", "values": {"values": "1, 2, 3"}}, "m": {"kind": "t.mul", "values": {"b": 10}}}, [("v", "values", "m", "a")], ["v", "m"])
    assert r["ok"], r
    same(out(r, "v", "values"), [1.0, 2.0, 3.0])
    same(out(r, "m", "out"), [10.0, 20.0, 30.0])
    # every output of a mapped node is a list, single inputs broadcast
    assert out(r, "m", "mode") == ["plain", "plain", "plain"]


def test_two_vary_nodes_form_every_combination_and_one_zips_with_itself():
    nodes = {"a": {"kind": "vary.int", "values": {"values": "1, 2"}}, "b": {"kind": "vary.float", "values": {"values": "10, 20, 30"}}, "m": {"kind": "t.mul"}}
    r = go(nodes, [("a", "values", "m", "a"), ("b", "values", "m", "b")], ["m"])
    # the first varies slowest
    assert out(r, "m", "out") == [10.0, 20.0, 30.0, 20.0, 40.0, 60.0]
    r = go(nodes, [("b", "values", "m", "a"), ("b", "values", "m", "b")], ["m"])
    assert out(r, "m", "out") == [100.0, 400.0, 900.0], "one list on two inputs zips"
    # a mapped node's list meets its source again downstream: they share the axis, so zip
    nodes = {"b": {"kind": "vary.float", "values": {"values": "1, 2, 3"}}, "m": {"kind": "t.mul", "values": {"b": 2}}, "n": {"kind": "t.mul"}}
    r = go(nodes, [("b", "values", "m", "a"), ("m", "out", "n", "a"), ("b", "values", "n", "b")], ["n"])
    assert out(r, "n", "out") == [2.0, 8.0, 18.0]


def test_a_slice_picks_from_the_list_a_wire_carries():
    nodes = {"v": {"kind": "vary.float", "values": {"values": "1, 2, 3, 4"}}, "m": {"kind": "t.mul", "values": {"b": 10}}, "s": {"kind": "t.sum"}}
    same(out(go(nodes, [("v", "values", "m", "a|1:")], ["m"]), "m", "out"), [20.0, 30.0, 40.0])
    assert out(go(nodes, [("v", "values", "m", "a|-1")], ["m"]), "m", "out") == 40.0
    assert out(go(nodes, [("v", "values", "s", "xs|::2")], ["s"]), "s", "sum") == 4.0
    same(out(go(nodes, [("v", "values", "m", "a| 1 : -1 ")], ["m"]), "m", "out"), [20.0, 30.0])
    same(out(go(nodes, [("v", "values", "m", "a|::-1")], ["m"]), "m", "out"), [40.0, 30.0, 20.0, 10.0])
    for bad, why in [("a|9", "out of range"), ("a|5:", "is empty"), ("a|x", "use start:stop:step"), ("a|3:1", "always empty")]:
        n = go(nodes, [("v", "values", "m", bad)], ["m"])["nodes"]["m"]
        assert n["state"] == "error" and n["error"]["kind"] == NodeError.CONFIG and why in n["error"]["message"], bad
    r = go({"m": {"kind": "t.mul"}, "n": {"kind": "t.mul"}}, [("m", "out", "n", "a|0")], ["n"])
    assert "needs a list" in r["nodes"]["n"]["error"]["message"]


def test_a_slice_is_part_of_the_memo_key():
    e, memo = engine(), exec.Memo()
    nodes = {"v": {"kind": "vary.float", "values": {"values": "1, 2, 3"}}, "m": {"kind": "t.mul"}}
    run = lambda q: go_with(e, memo, nodes, [("v", "values", "m", q)], ["m"])[0]  # noqa: E731
    assert run("a|1:")["nodes"]["m"]["state"] == "done"
    assert run("a|1:")["nodes"]["m"]["state"] == "cached"
    assert run("a| 1 : ")["nodes"]["m"]["state"] == "cached"
    r = run("a|:1")
    assert r["nodes"]["m"]["state"] == "done"
    same(out(r, "m", "out"), [1.0])


def test_the_shared_slice_vectors_hold():
    with open(os.path.join(os.path.dirname(__file__), "../../crates/proto/fixtures/slices.json")) as f:
        vectors = json.load(f)
    for case in vectors["parse"]:
        try:
            got = slices.canonical(slices.parse(case["spec"]))
        except ValueError:
            got = None
        assert got == case["canonical"], case
    for case in vectors["pick"]:
        s = slices.parse(case["spec"])
        try:
            got = slices.picks(s, case["n"])
        except ValueError:
            got = None
        assert got == case["items"], case


def test_the_same_bytes_from_elsewhere_are_found_by_content():
    e, memo = engine(), exec.Memo()

    def run(a, b):
        nodes = {"p": {"kind": "t.mul", "values": {"a": a, "b": b}}, "m": {"kind": "t.mul", "values": {"b": 10}}}
        return go_with(e, memo, nodes, [("p", "out", "m", "a")], ["m"])[0]

    assert run(2.0, 3.0)["stats"]["computed"] == 2
    second = run(3.0, 2.0)
    assert (second["stats"]["computed"], second["stats"]["from_content"], second["stats"]["from_memory"]) == (1, 1, 0)
    m = second["nodes"]["m"]
    assert m["state"] == "cached" and m["memo"]["source"] == "content" and "content_key" in m["memo"]
    assert out(second, "m", "out") == 60.0
    assert run(3.0, 2.0)["stats"]["from_memory"] == 2
    other = run(3.0, 3.0)
    assert other["nodes"]["m"]["state"] == "done" and out(other, "m", "out") == 90.0


def test_a_list_input_takes_the_list_whole():
    r = go({"r": {"kind": "vary.range", "values": {"start": 0, "stop": 1, "num": 5}}, "s": {"kind": "t.sum"}}, [("r", "values", "s", "xs")], ["r", "s"])
    same(out(r, "r", "values"), [0.0, 0.25, 0.5, 0.75, 1.0])
    assert out(r, "s", "sum") == 2.5


def test_vary_range_2d_and_3d_zip_their_coordinates():
    nodes = {"p": {"kind": "vary.range2d", "values": {"start_x": 0, "stop_x": 1, "start_y": 10, "stop_y": 20, "num": 3}}, "m": {"kind": "t.mul"}}
    r = go(nodes, [("p", "x", "m", "a"), ("p", "y", "m", "b")], ["p", "m"])
    assert out(r, "p", "x") == [0.0, 0.5, 1.0]
    assert out(r, "p", "y") == [10.0, 15.0, 20.0]
    assert out(r, "m", "out") == [0.0, 7.5, 20.0], "points along the line, not every combination"
    # grid: every combination, x fastest
    nodes = {"p": {"kind": "vary.range2d", "values": {"start_x": 1, "stop_x": 2, "start_y": 10, "stop_y": 20, "num": 2, "grid": True}}, "m": {"kind": "t.mul"}}
    r = go(nodes, [("p", "x", "m", "a"), ("p", "y", "m", "b")], ["p", "m"])
    assert out(r, "p", "x") == [1.0, 2.0, 1.0, 2.0]
    assert out(r, "p", "y") == [10.0, 10.0, 20.0, 20.0]
    assert out(r, "m", "out") == [10.0, 20.0, 20.0, 40.0]
    r = go({"p": {"kind": "vary.range3d", "values": {"num": 3, "grid": True, "endpoint": False}}}, [], ["p"])
    assert len(out(r, "p", "z")) == 27
    assert out(r, "p", "x")[:3] == [0.0, 1.0 / 3.0, 2.0 / 3.0]
    r = go({"p": {"kind": "vary.range3d", "values": {"num": 11, "grid": True}}}, [], ["p"])
    assert not r["ok"] and "1000" in r["nodes"]["p"]["error"]["message"], r


def test_vary_choices_takes_the_choices_of_the_enum_it_feeds():
    def nodes(values):
        return {"c": {"kind": "vary.choices", "values": {"values": values}}, "m": {"kind": "t.mul"}}

    r = go(nodes(""), [("c", "values", "m", "mode")], ["c", "m"])
    assert out(r, "m", "mode") == ["plain", "bold", "loud"]
    r = go(nodes("loud, bold"), [("c", "values", "m", "mode")], ["m"])
    assert out(r, "m", "mode") == ["loud", "bold"]
    r = go(nodes("shouty"), [("c", "values", "m", "mode")], ["m"])
    assert "not among" in r["nodes"]["c"]["error"]["message"], r
    r = go(nodes(""), [], ["c"])
    assert "blank" in r["nodes"]["c"]["error"]["message"], r


def test_an_item_that_fails_fails_the_node_and_names_the_item():
    r = go({"v": {"kind": "vary.float", "values": {"values": "1, -2, 3"}}, "m": {"kind": "t.mul"}}, [("v", "values", "m", "a")], ["m"])
    e = r["nodes"]["m"]["error"]
    assert e["kind"] == "config"
    assert e["message"].startswith("item 2 of 3:"), e
    r = go({"v": {"kind": "vary.float", "values": {"values": "1:5/0"}}}, [], ["v"])
    assert r["nodes"]["v"]["state"] == "error"


def test_a_watched_list_shows_one_item_with_a_sheet_of_labels():
    nodes = {"a": {"kind": "vary.int", "values": {"values": "1, 2"}}, "b": {"kind": "vary.float", "values": {"values": "0.5, 0.25"}}, "m": {"kind": "t.mul"}}
    wires = [("a", "values", "m", "a"), ("b", "values", "m", "b")]
    r, log = go_with(engine(), exec.Memo(), nodes, wires, ["m"], [{"node": "m", "port": "out", "index": -1}])
    d = r["watch"][0]
    assert (d["items"], d["index"]) == (4, 3)
    assert d["value"] == 0.5
    assert d["summary"].startswith("item 4 of 4 (a 2 · b 0.25)"), d["summary"]
    assert [c["label"] for c in d["sheet"]] == ["a 1 · b 0.5", "a 1 · b 0.25", "a 2 · b 0.5", "a 2 · b 0.25"]
    assert all("thumb" not in c for c in d["sheet"]), "thumbs only when the watch asks for a sheet"
    # progress per item, scaled into the whole
    fractions = [x["fraction"] for x in log if x["node_id"] == "m" and x["state"] == "progress"]
    assert fractions and all(0.0 <= f < 1.0 for f in fractions), fractions
    stages = [x["stage"] for x in log if x["node_id"] == "m" and x["state"] == "progress"]
    assert stages[0] == "item 1 of 4"


def test_a_mapped_node_is_memoised_whole_and_cancel_stops_between_items():
    e = engine()
    memo = exec.Memo()
    nodes = {"v": {"kind": "vary.float", "values": {"values": "1, 2, 3"}}, "m": {"kind": "t.mul"}}
    wires = [("v", "values", "m", "a")]
    first, _ = go_with(e, memo, nodes, wires, ["m"])
    again, _ = go_with(e, memo, nodes, wires, ["m"])
    assert again["nodes"]["m"]["state"] == "cached"
    assert out(again, "m", "out") == out(first, "m", "out")
    cancel = CancelToken()
    cancel.cancel()
    r, _ = go_with(e, exec.Memo(), nodes, wires, ["m"], cancel=cancel)
    assert not r["ok"]


# Python-side details the Rust tests take for granted.


def test_vary_choices_memo_key_follows_the_choices_it_feeds():
    e = engine()
    memo = exec.Memo()
    nodes = {"c": {"kind": "vary.choices", "values": {"values": "plain, bold"}}, "m": {"kind": "t.mul"}}
    r1, _ = go_with(e, memo, nodes, [("c", "values", "m", "mode")], ["c"])
    r2, _ = go_with(e, memo, nodes, [], ["c"])
    assert r1["nodes"]["c"]["memo"]["plan_key"] != r2["nodes"]["c"]["memo"]["plan_key"]


def test_lists_of_different_lengths_on_one_axis_fail_as_config():
    with pytest.raises(NodeError, match="list inputs differ in length: the lists is 2 long"):
        exec._mapping_plan([("a", List([1, 2])), ("b", List([1, 2, 3]))])
    axes, picks = exec._mapping_plan([("a", List([1, 2], [("x", 2)])), ("b", List([1, 2, 3], [("y", 3)]))])
    assert axes == [("x", 2), ("y", 3)] and picks[:4] == [[0, 0], [0, 1], [0, 2], [1, 0]]
    with pytest.raises(NodeError, match="more than 100000 runs"):
        exec._mapping_plan([("a", List([0] * 1000, [("x", 1000)])), ("b", List([0] * 1000, [("y", 1000)]))])


def test_a_raising_item_is_an_op_error_naming_the_item():
    class Boom(Node):
        definition = {"id": "t.boom", "inputs": [{"name": "a", "type": "FLOAT"}], "outputs": [{"name": "out", "type": "FLOAT"}]}

        def run(self, inputs, cx):
            if inputs["a"] > 1:
                raise ZeroDivisionError("no")
            return {"out": inputs["a"]}

    e = engine().with_node(Boom())
    r, _ = go_with(e, exec.Memo(), {"v": {"kind": "vary.float", "values": {"values": "1, 2"}}, "b": {"kind": "t.boom"}}, [("v", "values", "b", "a")], ["b"])
    assert r["nodes"]["b"]["error"] == {"kind": "op", "message": "item 2 of 2: ZeroDivisionError: no"}


def test_a_node_returning_lists_per_run_gives_one_flat_labelled_list():
    class Pair(Node):
        definition = {"id": "t.pair", "inputs": [{"name": "a", "type": "FLOAT"}], "outputs": [{"name": "out", "type": "FLOAT", "list": True}]}

        def run(self, inputs, cx):
            return {"out": List([inputs["a"], -inputs["a"]], labels=["plus", "minus"])}

    e = engine().with_node(Pair())
    nodes = {"v": {"kind": "vary.int", "values": {"values": "1, 2"}}, "p": {"kind": "t.pair"}}
    r, _ = go_with(e, exec.Memo(), nodes, [("v", "values", "p", "a")], ["p"], [{"node": "p", "port": "out"}])
    assert out(r, "p", "out") == [1, -1, 2, -2]
    assert [c["label"] for c in r["watch"][0]["sheet"]] == ["a 1 · plus", "a 1 · minus", "a 2 · plus", "a 2 · minus"]


def test_short_number():
    assert exec.short_number(0.5) == "0.5"
    assert exec.short_number(1.0) == "1"
    assert exec.short_number(1 / 3) == "0.333333"
    assert exec.short_number(-0.0000001) == "0"
    assert exec.short_number(3) == "3"
    assert exec.short_number(True) == "true"
    assert math.isclose(float(exec.short_number(2.25)), 2.25)


def test_hostile_vary_inputs_never_raise_anything_but_node_errors():
    e = engine()
    junk = [None, "", "::", "1:2:3:4", "1/2", ":1", "1:", "1:2/", "1:2/x", "inf:1", "1e400", "1:2/99999999999999999999999", [], {}, 1.5, True, -1, 10**30]
    for kind in ("vary.float", "vary.int", "vary.string", "vary.bool", "vary.choices", "vary.range", "vary.range2d", "vary.range3d"):
        node = e.nodes[kind]
        for v in junk:
            for key in ("values", "num", "start", "stop", "start_x", "endpoint", "grid", "_choices"):
                try:
                    node.run({key: v}, _Cx())
                except NodeError:
                    pass


class _Cx:
    node_id = "n"

    def cancelled(self):
        return False

    def progress(self, fraction, stage=None):
        pass


# Vary Range 2D's tuples, Zip, Outer Join, Unzip (crates/engine/tests/vary.rs).


def test_vary_range_2d_gives_points_as_tuples_that_unzip_back():
    nodes = {
        "p": {"kind": "vary.range2d", "values": {"start_x": 0, "stop_x": 1, "start_y": 10, "stop_y": 20, "num": 3}},
        "u": {"kind": "seq.unzip"},
        "m": {"kind": "t.mul"},
    }
    r = go(nodes, [("p", "xy", "u", "tuples"), ("u", "a", "m", "a"), ("u", "b", "m", "b")], ["p", "u", "m"])
    assert r["ok"], r
    assert out(r, "p", "xy") == [[0.0, 10.0], [0.5, 15.0], [1.0, 20.0]]
    assert out(r, "u", "a") == [0.0, 0.5, 1.0]
    assert out(r, "m", "out") == [0.0, 7.5, 20.0], "unzipped parts zip downstream"
    r = go({"p": {"kind": "vary.range3d", "values": {"num": 2}}}, [], ["p"])
    assert out(r, "p", "xyz") == [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]


def test_zip_pairs_in_order_and_outer_join_makes_every_combination():
    def nodes(kind, b):
        return {
            "a": {"kind": "vary.int", "values": {"values": "1, 2"}},
            "b": {"kind": "vary.string", "values": {"values": b}},
            "z": {"kind": kind},
            "u": {"kind": "seq.unzip"},
        }

    wires = [("a", "values", "z", "a"), ("b", "values", "z", "b"), ("z", "tuples", "u", "tuples")]
    r = go(nodes("seq.zip", "x, y"), wires, ["z", "u"])
    assert out(r, "z", "tuples") == [[1, "x"], [2, "y"]]
    assert out(r, "u", "b") == ["x", "y"]
    r = go(nodes("seq.zip", "x, y, z"), wires, ["z"])
    assert "differ in length" in r["nodes"]["z"]["error"]["message"], r
    r = go(nodes("seq.outer", "x, y, z"), wires, ["z", "u"])
    assert out(r, "z", "tuples") == [[1, "x"], [1, "y"], [1, "z"], [2, "x"], [2, "y"], [2, "z"]]
    assert out(r, "u", "a") == [1, 1, 1, 2, 2, 2]
    # a single value goes with every item
    nodes3 = {"a": {"kind": "vary.int", "values": {"values": "1, 2, 3"}}, "z": {"kind": "seq.zip"}, "t": {"kind": "t.mul", "values": {"a": 7}}}
    r = go(nodes3, [("a", "values", "z", "a"), ("t", "out", "z", "b")], ["z"])
    assert out(r, "z", "tuples") == [[1, 7.0], [2, 7.0], [3, 7.0]]
    # tuples are labelled by their parts
    r, _ = go_with(engine(), exec.Memo(), nodes("seq.outer", "x, y"), wires, ["z"], [{"node": "z", "port": "tuples", "index": 1}])
    assert [c["label"] for c in r["watch"][0]["sheet"]] == ["a 1 · b x", "a 1 · b y", "a 2 · b x", "a 2 · b y"]
    assert r["watch"][0]["value"] == [1, "y"]
    assert r["watch"][0]["summary"] == "item 2 of 4 (a 1 · b y): (1, y)"


def test_a_tuple_of_numbers_labels_a_run_and_unzip_handles_odd_input():
    nodes = {"p": {"kind": "vary.range2d", "values": {"num": 2}}, "m": {"kind": "t.sum"}}
    lst = List([List([0.0, 0.5]), List([1.0, 1.0])])
    assert exec.item_label("xy", lst, 0) == "xy (0, 0.5)"
    assert exec.item_label("xy", List([List([1, "a"])]), 0) == ""
    assert nodes  # the shape the Rust suite uses
    u = engine().nodes["seq.unzip"]
    with pytest.raises(NodeError, match="the list is empty"):
        u.run({"tuples": List([])}, _Cx())
    with pytest.raises(NodeError, match="a tuple is empty"):
        u.run({"tuples": List([List([]), List([1])])}, _Cx())
    with pytest.raises(NodeError, match="wire a list of tuples"):
        u.run({"tuples": 3}, _Cx())
    assert u.run({"tuples": List([1, "x"])}, _Cx()) == {"a": 1, "b": "x"}


# Select, Find, Load and Write Files (crates/engine/tests/vary.rs, files.rs).


def tmp(base):
    (base / "sub").mkdir(parents=True, exist_ok=True)
    return base


def test_find_files_lists_a_folder_and_notices_a_new_file(tmp_path):
    d = tmp(tmp_path)
    for name, body in [("b.txt", "bee"), ("a.TXT", "ay"), ("c.png", "x"), (".hidden.txt", "h"), ("sub/d.txt", "dee")]:
        (d / name).write_text(body)
    e = engine()
    memo = exec.Memo()

    def find(recursive):
        return {"f": {"kind": "file.find", "values": {"folder": str(d), "pattern": "*.txt", "recursive": recursive}}, "l": {"kind": "file.load"}}

    wires = [("f", "files", "l", "file")]
    r, _ = go_with(e, memo, find(False), wires, ["f", "l"], [{"node": "l", "port": "value"}])
    assert r["ok"], r
    assert out(r, "f", "files") == [str(d / "a.TXT"), str(d / "b.txt")]
    assert r["watch"][0]["items"] == 2
    assert [c["label"] for c in r["watch"][0]["sheet"]] == ["a", "b"]
    assert r["watch"][0]["media"] == "bytes" and r["watch"][0]["summary"] == "item 1 of 2 (a): text/plain, 2 bytes"
    r, _ = go_with(e, memo, find(True), wires, ["f"])
    assert len(out(r, "f", "files")) == 3
    # unchanged: cached; a new file: found
    r, _ = go_with(e, memo, find(False), wires, ["f"])
    assert r["nodes"]["f"]["state"] == "cached"
    (d / "e.txt").write_text("e")
    r, _ = go_with(e, memo, find(False), wires, ["f"])
    assert r["nodes"]["f"]["state"] == "done"
    assert len(out(r, "f", "files")) == 3
    r = go({"f": {"kind": "file.find", "values": {"folder": str(d), "pattern": "*.nope"}}}, [], ["f"])
    assert "no files match" in r["nodes"]["f"]["error"]["message"]


def test_select_load_and_write_files_name_each_file_by_what_made_it(tmp_path):
    d = tmp(tmp_path)
    (d / "corgi.svg").write_text("<svg/>")
    (d / "plum.svg").write_text("<svg></svg>")
    outdir = d / "out"
    files = f"{d / 'corgi.svg'}\n{d / 'plum.svg'}"

    def nodes(overwrite):
        return {
            "s": {"kind": "file.select", "values": {"files": files}},
            "l": {"kind": "file.load"},
            "w": {"kind": "file.write", "values": {"folder": str(outdir), "basename": "copy", "overwrite": overwrite}},
        }

    wires = [("s", "files", "l", "file"), ("l", "value", "w", "values")]
    r = go(nodes(False), wires, ["w"])
    assert r["ok"], r
    assert out(r, "w", "files") == [str(outdir / "copy_corgi.svg"), str(outdir / "copy_plum.svg")]
    assert (outdir / "copy_plum.svg").read_text() == "<svg></svg>"
    # the same again: nothing to replace
    assert go(nodes(False), wires, ["w"])["ok"]
    # a different file there: refused unless overwrite
    (outdir / "copy_plum.svg").write_text("changed")
    r = go(nodes(False), wires, ["w"])
    assert "overwrite" in r["nodes"]["w"]["error"]["message"], r
    assert (outdir / "copy_plum.svg").read_text() == "changed", "nothing written"
    assert go(nodes(True), wires, ["w"])["ok"]
    assert (outdir / "copy_plum.svg").read_text() == "<svg></svg>"
    # a swept setting names the files too; plain values are written as text or JSON
    r = go(
        {
            "v": {"kind": "vary.float", "values": {"values": "0.5, 2"}},
            "m": {"kind": "t.mul", "values": {"b": 2}},
            "w": {"kind": "file.write", "values": {"folder": str(outdir), "basename": "m"}},
        },
        [("v", "values", "m", "a"), ("m", "out", "w", "values")],
        ["w"],
    )
    assert out(r, "w", "files") == [str(outdir / "m_a-0.5.json"), str(outdir / "m_a-2.json")]
    assert (outdir / "m_a-2.json").read_text() == "4.0"
    r = go({"s": {"kind": "file.select", "values": {"files": ""}}}, [], ["s"])
    assert "no files chosen" in r["nodes"]["s"]["error"]["message"]


def test_globs_and_names():
    from plumcraft_engine.files import file_stem, glob_match

    assert glob_match("*.PNG", "corgi.png")
    assert glob_match("c?rgi*", "corgi-2.jpg")
    assert not glob_match("*.png", "corgi.png.txt")
    assert glob_match("*", "")
    assert not glob_match("a*b*c", "aXbY")
    assert file_stem("trace", "corgi · colors 8") == "trace_corgi_colors-8"
    assert file_stem("", "sub/dir/a b · x 0.5") == "sub_dir_a-b_x-0.5"
    assert file_stem("out", "") == "out"


def test_media_types_both_ways():
    from plumcraft_engine.media import TABLE, ext_for, media_for_ext

    assert ext_for("Video/MP4; codecs=avc1") == "mp4"
    assert ext_for("x/unknown") == "bin"
    assert media_for_ext("JPEG") == "image/jpeg"
    assert media_for_ext(".svg") == "image/svg+xml"
    assert media_for_ext("weird") == "application/octet-stream"
    for t, e in TABLE:
        assert ext_for(media_for_ext(e)) == ext_for(t), (t, e)


def test_any_brings_conversions_to_and_from_every_type():
    s = engine().schema()
    assert s["compat"][:3] == [{"from": "INT", "to": "FLOAT"}, {"from": "STRING", "to": "ENUM"}, {"from": "ENUM", "to": "STRING"}]
    assert {"from": "FLOAT", "to": "ANY"} in s["compat"] and {"from": "ANY", "to": "BOOL"} in s["compat"]
    assert "compat" not in Engine({"id": "x"}).with_vary().schema()


def test_hostile_building_block_inputs_never_raise_anything_but_node_errors():
    e = engine()
    junk = [None, "", "~", "\x00", "/nonexistent/zz", [], {}, 1.5, True, List([]), List([List([])]), List([1, List([2])])]
    for kind in ("seq.zip", "seq.outer", "seq.unzip", "file.select", "file.find", "file.load"):
        node = e.nodes[kind]
        for v in junk:
            for key in ("a", "b", "tuples", "files", "folder", "file", "pattern"):
                try:
                    node.run({key: v}, _Cx())
                    node.volatile({key: v})
                except NodeError:
                    pass


def test_table_to_rows():
    """Table to Rows (as `crates/engine/src/seq.rs`): named columns as rows, the names first;
    a table array likewise (its channel names, else c1, c2); rows pass through; bad input is a
    config error."""
    import numpy as np
    import pytest

    from plumcraft_engine import ArrayValue, NodeError
    from plumcraft_engine.seq import array_rows, table_rows

    t = {"columns": [{"name": "letter", "values": ["a", "b"]}, {"name": "count", "values": [3, 1]}]}
    assert table_rows(t) == [["letter", "count"], ["a", 3], ["b", 1]]
    assert table_rows(t, header=False) == [["a", 3], ["b", 1]]
    assert table_rows([[1, 2], [3, 4]]) == [[1, 2], [3, 4]]
    a = ArrayValue(np.array([[0.25, 2.0], [0.75, np.nan]], dtype=np.float32), None, ["centre", "count"], True)
    assert array_rows(a) == [["centre", "count"], [0.25, 2.0], [0.75, None]]
    assert array_rows(np.zeros((1, 2), np.float32))[0] == ["c1", "c2"]
    for bad in ({"rows": []}, {"columns": [{"name": "x"}]}, 7):
        with pytest.raises(NodeError):
            table_rows(bad)
    with pytest.raises(NodeError):
        array_rows(np.zeros((2, 2, 2), np.float32))


def test_rows_to_table():
    """Rows to Table (as the Rust test): names from the first row, types from the values, and a
    round trip through Table to Rows."""
    import pytest

    from plumcraft_engine import NodeError
    from plumcraft_engine.seq import rows_table, table_rows

    rows = [["letter", "count", "share", "ok"], ["a", 3, 0.5, True], ["b", 1, 1, None]]
    t = rows_table(rows)
    assert t == {
        "columns": [
            {"name": "letter", "type": "STRING", "values": ["a", "b"]},
            {"name": "count", "type": "INT", "values": [3, 1]},
            {"name": "share", "type": "FLOAT", "values": [0.5, 1]},
            {"name": "ok", "type": "BOOL", "values": [True, None]},
        ]
    }
    assert table_rows(t) == rows
    assert rows_table([[1, 2]], header=False)["columns"][1]["name"] == "c2"
    assert rows_table(t) == t
    for bad in (7, [1, 2], {"rows": []}):
        with pytest.raises(NodeError):
            rows_table(bad)
