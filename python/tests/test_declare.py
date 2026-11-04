"""The declarative layer: each annotation kind to its exact port, the mistakes caught when a class
is defined, value checking from ranges and choices, and a small engine served end to end."""

import json
import pathlib
from dataclasses import dataclass, field
from typing import Annotated, Literal, NamedTuple

import pytest

import plumcraft_engine
from plumcraft_engine import (
    ConfigError,
    Cx,
    DeclarationError,
    Engine,
    File,
    Float,
    Int,
    NodeError,
    Port,
    Range,
    String,
    WireType,
    node,
    simple,
)
from plumcraft_engine.host import Host
from plumcraft_text import TextEngine

Text = Annotated[str, WireType("TEXT", media="text", color="#c9a227", thumb=True)]
Audio = Annotated[object, WireType("AUDIO", media="audio")]


def ports(cls: type[Engine], node_id: str) -> tuple[list[dict], list[dict]]:
    d = cls().nodes[node_id].definition
    return d["inputs"], d["outputs"]


class Kinds(Engine):
    id = "k"
    name = "Kinds"

    @node
    def scalars(self, s: str = "x", i: int = 3, f: float = 2, b: bool = True) -> None: ...

    @node
    def sockets(self, t: Text, a: Audio, maybe: Text | None = None) -> Audio: ...

    @node
    def enum(self, mode: Literal["over", "multiply"] = "over") -> None: ...

    @node
    def ranged(self, x: Annotated[float, Range(0, 1, 0.25)] = 0.5, n: Annotated[int, Range(min=1)] = 1) -> None: ...

    @node
    def files(self, src: Annotated[str, File("open", (("Text", ("txt", "md")),))] = "", dst: Annotated[str, File("save")] = "") -> None: ...

    @node
    def presented(
        self,
        with_: Annotated[str, Port(label="With", tooltip="the new text", advanced=True)] = "",
        w: Annotated[Float, Port(socket="widget")] = 1.0,
        sql: Annotated[str, Port(widget="multiline")] = "SELECT 1",
    ) -> None: ...

    @node(output="n")
    def counted(self, t: Text) -> int:
        return len(t)


def test_scalars_are_widgets_with_typed_defaults():
    inputs, outputs = ports(Kinds, "k.scalars")
    assert inputs == [
        {"name": "s", "type": "STRING", "default": "x"},
        {"name": "i", "type": "INT", "default": 3},
        {"name": "f", "type": "FLOAT", "default": 2.0},
        {"name": "b", "type": "BOOL", "default": True},
    ]
    assert json.dumps(inputs[2]["default"]) == "2.0", "an int default on a FLOAT port is a float"
    assert outputs == []


def test_wire_types_are_sockets_and_none_is_optional():
    inputs, outputs = ports(Kinds, "k.sockets")
    assert inputs == [
        {"name": "t", "type": "TEXT", "socket": "socket"},
        {"name": "a", "type": "AUDIO", "socket": "socket"},
        {"name": "maybe", "type": "TEXT", "optional": True, "socket": "socket"},
    ]
    assert outputs == [{"name": "audio", "type": "AUDIO"}]


def test_literal_is_enum_with_choices():
    assert ports(Kinds, "k.enum")[0] == [{"name": "mode", "type": "ENUM", "default": "over", "choices": ["over", "multiply"]}]


def test_range_file_and_port_metadata():
    assert ports(Kinds, "k.ranged")[0] == [
        {"name": "x", "type": "FLOAT", "default": 0.5, "min": 0.0, "max": 1.0, "step": 0.25},
        {"name": "n", "type": "INT", "default": 1, "min": 1.0},
    ]
    assert ports(Kinds, "k.files")[0] == [
        {"name": "src", "type": "STRING", "default": "", "path": {"mode": "open", "filters": [{"name": "Text", "extensions": ["txt", "md"]}]}},
        {"name": "dst", "type": "STRING", "default": "", "path": {"mode": "save"}},
    ]
    assert ports(Kinds, "k.presented")[0] == [
        {"name": "with", "type": "STRING", "label": "With", "tooltip": "the new text", "default": "", "advanced": True},
        {"name": "w", "type": "FLOAT", "default": 1.0, "socket": "widget"},
        {"name": "sql", "type": "STRING", "default": "SELECT 1", "widget": "multiline"},
    ]


def test_node_defaults_and_wire_types_in_first_use_order():
    e = Kinds()
    d = e.nodes["k.counted"].definition
    assert d == {
        "id": "k.counted",
        "display": "Counted",
        "inputs": [{"name": "t", "type": "TEXT", "socket": "socket"}],
        "outputs": [{"name": "n", "type": "INT"}],
    }
    assert [w["id"] for w in e.wire_types] == ["STRING", "INT", "FLOAT", "BOOL", "TEXT", "AUDIO", "ENUM"]
    assert e.wire_types[4] == {"id": "TEXT", "color": "#c9a227", "media": "text", "thumb": True}
    assert {k: v for k, v in e.info.items() if k != "salt"} == {"id": "k", "name": "Kinds", "version": "0.0.0"}
    assert e.info["salt"].startswith("py:k+0.0.0+code:") and "+sdk:" in e.info["salt"], "the code is in the salt"


class Pair(NamedTuple):
    head: Text
    size: int


@dataclass
class Look:
    """Settings as a dataclass, as Cartooner's plugins declare theirs."""

    opacity: Annotated[float, Range(0, 1)] = 1.0
    blend: Literal["over", "add"] = "over"
    tags: str = field(default_factory=lambda: "a,b")


class Shapes(Engine):
    id = "shape"
    category = ("Shapes", "Test")
    family = "shapes"
    icon = "star"

    @node("Split", version=2, memo="none", flags={"result": True}, x={"demo": {"v": 1}})
    def split(self, t: Text) -> Pair:
        """Head and size.

        Two outputs from a NamedTuple."""
        return Pair(t[:1], len(t))

    @node(output=("first", "count"))
    def renamed(self, t: Text) -> Pair:
        return Pair(t[:1], len(t))

    @node(id="shape.look", description="explicit", category="Other")
    def styled(self, t: Text, look: Look) -> Text:
        return f"{t}:{look.blend}:{look.opacity}:{look.tags}"


def test_multiple_outputs_docstrings_and_node_options():
    d = Shapes().nodes["shape.split"].definition
    assert d == {
        "id": "shape.split",
        "display": "Split",
        "category": ["Shapes", "Test"],
        "description": "Head and size.\n\nTwo outputs from a NamedTuple.",
        "inputs": [{"name": "t", "type": "TEXT", "socket": "socket"}],
        "outputs": [{"name": "head", "type": "TEXT"}, {"name": "size", "type": "INT"}],
        "flags": {"result": True},
        "family": "shapes",
        "icon": "star",
        "version": 2,
        "memo": {"scope": "none"},
        "x": {"demo": {"v": 1}},
    }
    assert [p["name"] for p in Shapes().nodes["shape.renamed"].definition["outputs"]] == ["first", "count"]
    look = Shapes().nodes["shape.look"].definition
    assert look["category"] == ["Other"] and look["description"] == "explicit"
    assert look["inputs"][1:] == [
        {"name": "opacity", "type": "FLOAT", "default": 1.0, "min": 0.0, "max": 1.0},
        {"name": "blend", "type": "ENUM", "default": "over", "choices": ["over", "add"]},
        {"name": "tags", "type": "STRING", "default": "a,b"},
    ]


def test_calls_map_inputs_to_arguments_and_results_to_outputs():
    e = Shapes()
    assert e.nodes["shape.split"].run({"t": "corgi"}, None) == {"head": "c", "size": 5}
    assert e.nodes["shape.look"].run({"t": "x", "opacity": 0.5, "blend": "add", "tags": "c"}, None) == {"text": "x:add:0.5:c"}


def test_the_text_engine_catalogue_is_unchanged():
    """The parity contract: the declared text engine says exactly what the hand-written one did."""
    e = TextEngine()
    salt = e.info["salt"]
    golden = json.loads((pathlib.Path(__file__).parent / "golden" / "text_schema.json").read_text())
    # The salt carries the code digest (it changes with the code, by design): its prefix is fixed.
    assert salt.startswith(golden["engine"]["salt"] + "+code:"), salt
    got = json.dumps({**e.schema(), "engine": {**e.info, "salt": golden["engine"]["salt"]}}, indent=1, ensure_ascii=False) + "\n"
    assert got == json.dumps(golden, indent=1, ensure_ascii=False) + "\n"


# Class-definition-time errors.


def declare(body: dict, wire_types=None) -> type:
    ns = dict(body, id="bad")
    if wire_types is not None:
        ns["wires"] = wire_types
    return type("Bad", (Engine,), ns)


def test_missing_return_annotation():
    def f(self, t: Text):
        return t

    with pytest.raises(DeclarationError, match=r"Bad\.f \(bad\.f\): needs a return annotation"):
        declare({"f": node(f)})


def test_definition_time_errors():
    def unknown(self, x: list) -> None: ...

    def out_of_range(self, x: Annotated[float, Range(0, 1)] = 2.0) -> None: ...

    def not_a_choice(self, m: Literal["a", "b"] = "c") -> None: ...

    def wrong_default(self, n: int = "three") -> None: ...

    def unannotated(self, n=1) -> None: ...

    def var(self, *xs: int) -> None: ...

    def range_on_str(self, s: Annotated[str, Range(0, 1)] = "") -> None: ...

    def unknown_extra(self, s: Annotated[str, "label?"] = "") -> None: ...

    def none_default(self, s: str = None) -> None: ...  # type: ignore[assignment]

    def twin(self, a: int = 0, a_: int = 0) -> None: ...

    def bad_out(self) -> list: ...

    def ok():  # a fresh function each time: @node tags the function it is given
        def f(self) -> None: ...

        return f

    cases = [
        ({"f": node(unknown)}, "has no port meaning"),
        ({"f": node(out_of_range)}, r"default 2\.0 is outside 0\.\.1"),
        ({"f": node(not_a_choice)}, "is not one of"),
        ({"f": node(wrong_default)}, "is not an integer"),
        ({"f": node(unannotated)}, "needs an annotation"),
        ({"f": node(var)}, "variadics"),
        ({"f": node(range_on_str)}, "Range applies to int and float"),
        ({"f": node(unknown_extra)}, "unknown annotation"),
        ({"f": node(none_default)}, r"X \| None"),
        ({"f": node(twin)}, "two inputs named a"),
        ({"f": node(bad_out)}, "has no port meaning"),
        ({"f": node(ok()), "g": node(id="bad.f")(ok())}, "node id bad.f is also"),
        ({"schema": node(ok())}, "the Engine's own"),
        ({"name": node(ok())}, "the Engine's own"),
        ({"f": node(output=("a", "b"))(ok())}, "returns None"),
    ]
    for body, message in cases:
        with pytest.raises(DeclarationError, match=message):
            declare(body)


def test_wire_types_must_be_declared_once():
    def uses_text(self, t: Text) -> None: ...

    with pytest.raises(DeclarationError, match="TEXT is not in Bad.wires"):
        declare({"f": node(uses_text)}, wire_types=(String,))
    with pytest.raises(DeclarationError, match="listed twice"):
        declare({}, wire_types=(String, String))
    with pytest.raises(DeclarationError, match="is not a WireType"):
        declare({}, wire_types=(str,))
    other_text = Annotated[str, WireType("TEXT", media="text")]

    def both(self, a: Text, b: other_text) -> None: ...

    with pytest.raises(DeclarationError, match="declared twice, differently"):
        declare({"f": node(both)})


def test_decorating_something_other_than_a_function():
    with pytest.raises(DeclarationError, match="plain method"):
        node()(staticmethod(lambda: None))


# Values checked from the declared ranges and choices.


class Checked(Engine):
    id = "c"

    @node
    def strict(self, x: Annotated[float, Range(0, 10)] = 1.0, n: int = 0, m: Literal["a", "b"] = "a", on: bool = False) -> String:
        return f"{x}/{n}/{m}/{on}"

    @node
    def clamped(self, x: Annotated[float, Range(0, 10, invalid="clamp")] = 1.0, n: Annotated[int, Range(1, 5, invalid="clamp")] = 2) -> String:
        return f"{x}/{n}"

    @node
    def passed(self, x: Annotated[int, Range(0, 10, invalid="pass")] = 1) -> String:
        return repr(x)

    @node
    def texts(self, s: str = "", t: Text = "") -> String:
        return f"{s}|{t}"


def run_node(e: Engine, node_id: str, inputs: dict):
    return e.nodes[node_id].run(inputs, None)


def test_strict_values_fail_as_config_errors():
    e = Checked()
    assert run_node(e, "c.strict", {"x": 10, "n": 3, "m": "b", "on": True}) == {"string": "10.0/3/b/True"}
    for inputs, message in [
        ({"x": 10.5}, "x: 10.5 is outside 0..10"),
        ({"x": "big"}, 'x: expected a number, got "big"'),
        ({"x": True}, "x: expected a number, got true"),
        ({"n": 1.5}, "n: expected an integer, got 1.5"),
        ({"m": "c"}, 'm: expected one of "a", "b", got "c"'),
        ({"on": 1}, "on: expected true or false, got 1"),
    ]:
        with pytest.raises(ConfigError) as err:
            run_node(e, "c.strict", {"x": 1.0, "n": 0, "m": "a", "on": False} | inputs)
        assert err.value.kind == NodeError.CONFIG and err.value.message == message


def test_clamp_clamps_and_replaces_wrong_types_with_the_default():
    e = Checked()
    assert run_node(e, "c.clamped", {"x": 11.0, "n": 0}) == {"string": "10.0/1"}
    assert run_node(e, "c.clamped", {"x": -1.0, "n": 9}) == {"string": "0.0/5"}
    assert run_node(e, "c.clamped", {"x": "x", "n": None}) == {"string": "1.0/None"}
    assert run_node(e, "c.clamped", {"x": False, "n": 2.5}) == {"string": "1.0/2"}


def test_pass_hands_the_value_over_as_it_is_and_text_ports_take_any_value_as_text():
    e = Checked()
    assert run_node(e, "c.passed", {"x": -7}) == {"string": "-7"}
    assert run_node(e, "c.passed", {"x": "seven"}) == {"string": "'seven'"}
    assert run_node(e, "c.texts", {"s": 12, "t": {"a": [1, "é"]}}) == {"string": '12|{"a":[1,"é"]}'}


# A tiny engine, declared here, served through the host and run end to end.


class Shout(Engine):
    id = "shout"
    name = "Shout"
    version = "1.2.3"
    wires = (Text, String, Int)

    @node("Say")
    def say(self, words: str = "woof") -> Text:
        return words

    @node("Loud")
    def loud(self, t: Text, marks: Annotated[int, Range(0, 5)] = 1) -> Text:
        return t.upper() + "!" * marks

    @node("Wait")
    def wait(self, t: Text, *, cx: Cx) -> Text:
        cx.progress(0.5, "half")
        if cx.cancelled():
            raise NodeError.cancelled()
        return t


def reply_of(host, run_id, timeout=10.0):
    import time

    deadline, since = time.monotonic() + timeout, 0
    events = []
    while time.monotonic() < deadline:
        p = host.poll(since, 200, run_id)
        since = p["last_seq"]
        for e in p["events"]:
            events.append(e)
            if e["kind"] == "run.reply":
                return e["body"], events
    raise AssertionError(f"no reply for {run_id}")


def graph(**values):
    return {
        "nodes": {"a": {"kind": "shout.say", "values": {"words": "the corgi"}}, "w": {"kind": "shout.wait"}, "b": {"kind": "shout.loud", "values": values}},
        "wires": [
            {"from": {"node": "a", "port": "text"}, "to": {"node": "w", "port": "t"}},
            {"from": {"node": "w", "port": "text"}, "to": {"node": "b", "port": "t"}},
        ],
    }


def test_a_declared_engine_runs_end_to_end_through_the_host():
    h = Host(Shout)
    try:
        assert h.wait_ready(5)["state"] == "connected"
        schema = h.schema(None)
        assert [n["id"] for n in schema["nodes"]] == ["shout.loud", "shout.say", "shout.wait"]
        assert schema["engine"]["salt"].startswith("py:shout+1.2.3+code:")
        h.run({"v": 2, "op": "run", "run_id": "r", "generation": 1, "graph": {"kind": "inline", "graph": graph(marks=3)}, "targets": ["b"]})
        reply, events = reply_of(h, "r")
        assert reply["ok"] and reply["order"] == ["a", "w", "b"]
        assert reply["nodes"]["b"]["outputs"]["text"]["value"] == "THE CORGI!!!"
        assert any(e["kind"] == "node.status" and e["body"].get("stage") == "half" for e in events), "cx reaches the method"
        h.run({"v": 2, "op": "run", "run_id": "bad", "generation": 1, "graph": {"kind": "inline", "graph": graph(marks=9)}, "targets": ["b"]})
        reply, _ = reply_of(h, "bad")
        assert reply["nodes"]["b"]["error"] == {"kind": "config", "message": "marks: 9 is outside 0..5"}
    finally:
        h.shutdown()


def test_subclasses_inherit_nodes_and_the_low_level_api_still_mixes_in():
    class Louder(Shout):
        @node("Loud")
        def loud(self, t: Text, marks: Annotated[int, Range(0, 9)] = 2) -> Text:
            return t.upper() + "?" * marks

        def wait(self, t, cx):  # a plain override: no longer a node
            return t

    e = Louder().with_node(simple({"id": "shout.raw", "inputs": [], "outputs": [{"name": "text", "type": "TEXT"}]}, lambda i: {"text": "raw"}))
    assert sorted(e.nodes) == ["shout.loud", "shout.raw", "shout.say"]
    assert e.nodes["shout.loud"].definition["inputs"][1]["max"] == 9.0
    assert run_node(e, "shout.loud", {"t": "a", "marks": 2}) == {"text": "A??"}
    assert [w["id"] for w in e.wire_types] == ["TEXT", "STRING", "INT"]
    assert Engine({"id": "low", "name": "Low", "version": "1"}).info["id"] == "low"


def test_named_dependencies_join_the_salt():
    class Plain(Engine):
        id = "p"

    class Numeric(Engine):
        id = "p"
        dependencies = ("numpy",)

    class Absent(Engine):
        id = "p"
        dependencies = ("no-such-distribution",)

    assert "+deps:" not in Plain().info["salt"], "an engine that names none keeps its salt"
    a, b = Numeric().info["salt"], Absent().info["salt"]
    assert "+deps:" in a and "+deps:" in b and a.split("+deps:")[1] != b.split("+deps:")[1]
    assert plumcraft_engine.dependency_digest(("a", "b")) == plumcraft_engine.dependency_digest(("b", "a")), "order doesn't matter"


def test_a_new_dependency_version_changes_the_digest(monkeypatch):
    from importlib import metadata

    before = plumcraft_engine.dependency_digest(("numpy",))
    monkeypatch.setattr(metadata, "version", lambda dist: "999.0")
    assert plumcraft_engine.dependency_digest(("numpy",)) != before


def test_converts_marks_a_converter_and_checks_it():
    Wav = Annotated[object, WireType("WAV", media="audio")]

    class Conv(Engine):
        id = "conv"

        @node(converts=True, lossless=True)
        def decode(self, wav: Wav, gain: float = 1.0) -> Audio: ...

        @node(converts=("TEXT", "AUDIO"))
        def speak(self, text: Text) -> Audio: ...

    nodes = {n["id"]: n for n in Conv().schema()["nodes"]}
    assert nodes["conv.decode"]["converts"] == {"from": "WAV", "to": "AUDIO", "input": "wav", "output": "audio", "lossless": True}
    assert nodes["conv.speak"]["converts"] == {"from": "TEXT", "to": "AUDIO", "input": "text", "output": "audio"}

    with pytest.raises(DeclarationError, match="no wire-only TABLE input"):

        class Wrong(Engine):
            id = "wrong"

            @node(converts=("TABLE", "AUDIO"))
            def speak(self, text: Text) -> Audio: ...

    with pytest.raises(DeclarationError, match="converts must be"):
        node(converts="yes")
