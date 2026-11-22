"""Save Result in the Python SDK (`output.save`, spec 15; the maintainer, 2026-11-22: "Make the save
an basic feature"), as the Rust SDK's `crates/engine/tests/save.rs`: an engine whose nodes output
core types saves them with no code of its own; it adds, replaces and takes away formats and gives
a node its own list. Driven through the real protocol (the socket binding, JSON-RPC lines), and
the encoders' bytes checked against the Rust SDK's."""

import base64
import json
import os
import socket
import threading

import numpy as np
import pytest
import scratch

from plumcraft_engine import Array, AudioValue, Engine, List, NodeError, Text, node, save
from plumcraft_engine.serve import serve_unix


def _shout(v, cx):
    if not isinstance(v, str):
        raise NodeError(NodeError.CONFIG, "Shouted Text saves text")
    return (v.upper() + cx.options.get("mark", "!")).encode()


SHOUT = save.Format("shout", "Shouted Text", ("shout",), ("text",), "text/plain", _shout).with_option(
    save.option("mark", "Mark", "ENUM", "!", choices=["!", "?"])
)


class Saver(Engine):
    id = "t"
    name = "T"
    version = "1"
    save_formats = (SHOUT,)

    @node("Text", flags={"result": True})
    def text(self, word: str = "plum") -> Text:
        return word

    @node("Loud", save_formats=["shout", "txt"])
    def loud(self, word: str = "plum") -> Text:
        return word

    @node("Words")
    def words(self, word: str = "plum") -> Text:
        return List([word, "b"], labels=["one", "two"])

    @node("Pictures")
    def pics(self, word: str = "plum") -> Array:
        return List([np.full((2, 2), v, dtype=np.uint8) for v in (10, 200, 90)], labels=["a 1", "a 2", "a 3"])

    @node("Strip")
    def strip(self, word: str = "plum") -> Array:
        return np.full((4, 1000), 0.5, dtype=np.float32)

    @node("Fails")
    def fails(self) -> Text:
        raise NodeError(NodeError.OP, "it broke")


def _engine():
    return Saver().with_store(None)


class _Ui:
    """The UI end of the socket: listen, let the engine connect, talk JSON-RPC lines."""

    def __init__(self, factory, token="t0k"):
        kind = "unix" if hasattr(socket, "AF_UNIX") else "tcp"
        if kind == "unix":
            self.dir = scratch.mkdtemp("save-ui-")
            loc = os.path.join(self.dir, "e.sock")
            srv = socket.socket(socket.AF_UNIX)
            srv.bind(loc)
        else:
            srv = socket.socket(socket.AF_INET)
            srv.bind(("127.0.0.1", 0))
            loc = f"tcp:127.0.0.1:{srv.getsockname()[1]}"
        srv.listen(1)
        self.thread = threading.Thread(target=serve_unix, args=(factory, loc, token), daemon=True)
        self.thread.start()
        srv.settimeout(5)
        self.sock, _ = srv.accept()
        srv.close()
        self.sock.settimeout(30)
        self.f = self.sock.makefile("rb")
        self.next = 0
        hello = self.call("hello", {"v": 2, "op": "hello", "token": token, "protocol": {"min": 2, "max": 2}, "caps": ["events.poll"]})
        self.caps = hello["result"]["caps"]
        self.call("session.confirm", {"shm": False, "path": True})

    def call(self, method, params):
        self.next += 1
        self.sock.sendall((json.dumps({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params}) + "\n").encode())
        while True:
            line = self.f.readline()
            if not line:
                return None
            msg = json.loads(line)
            if msg.get("id") == self.next:
                return msg

    def save(self, kind, **extra):
        g = {"kind": "inline", "graph": {"version": 1, "nodes": {"n": {"kind": kind, "values": {"word": "plum"}}}, "wires": []}}
        return self.call("output.save", {"graph": g, "node": "n", **extra})["result"]

    def close(self):
        self.f.close()
        self.sock.close()
        self.thread.join(5)


@pytest.fixture
def ui():
    u = _Ui(_engine)
    yield u
    u.close()


def _inline(ref):
    return base64.b64decode(next(a["b64"] for a in ref["at"] if a["kind"] == "inline"))


def test_the_catalogue_lists_its_formats_and_hello_says_it_saves(ui):
    s = ui.call("schema", {})["result"]
    assert [f["id"] for f in s["save_formats"]] == ["txt", "png", "jpeg", "npy", "shout"], "TEXT and ARRAY defaults in order, then its own"
    shout = s["save_formats"][-1]
    assert shout["options"][0]["choices"] == ["!", "?"] and shout["extensions"] == ["shout"] and shout["media"] == ["text"]
    loud = next(n for n in s["nodes"] if n["id"] == "t.loud")
    assert loud["save_formats"] == ["shout", "txt"]
    assert "output.save" in ui.caps
    bare = _engine().without_default_save_formats().without_save_format("shout")
    assert "save_formats" not in bare.schema() and "output.save" not in bare.hello({"op": "hello", "caps": []}, "s")["caps"]
    plain = save.Format("txt", "Plain", ("text",), ("text",), "text/plain", lambda v, cx: b"x")
    assert next(f for f in _engine().with_save_format(plain).schema()["save_formats"] if f["id"] == "txt")["label"] == "Plain"


def test_a_save_writes_the_file_the_request_names(ui):
    d = scratch.mkdtemp("save-path-")
    r = ui.save("t.text", port="text", path=os.path.join(d, "w.txt"))
    assert r["ok"] and r["paths"] == [os.path.join(d, "w.txt")] and r["recipe"] is False, r
    assert open(os.path.join(d, "w.txt"), "rb").read() == b"plum"
    r = ui.save("t.loud", path=os.path.join(d, "w.shout"), options={"mark": "?"})
    assert r["ok"], r
    assert open(os.path.join(d, "w.shout"), "rb").read() == b"PLUM?"
    r = ui.save("t.loud", path=os.path.join(d, "noext"))
    assert r["ok"] and open(os.path.join(d, "noext"), "rb").read() == b"PLUM!", "the node's first format, its option's default"
    assert not [n for n in os.listdir(d) if n.startswith(".")], "no temporary file is left"


def test_a_list_saves_as_numbered_files_or_one_item(ui):
    d = scratch.mkdtemp("save-list-")
    r = ui.save("t.words", path=os.path.join(d, "w.txt"))
    assert r["ok"] and r["items"] == 2 and r["labels"] == ["one", "two"], r
    assert r["paths"] == [os.path.join(d, "w-00.txt"), os.path.join(d, "w-01.txt")]
    assert open(os.path.join(d, "w-01.txt"), "rb").read() == b"b"
    r = ui.save("t.words", path=os.path.join(d, "one.txt"), index=1)
    assert r["ok"] and open(os.path.join(d, "one.txt"), "rb").read() == b"b"
    assert ui.save("t.words", path=os.path.join(d, "x.txt"), index=2)["error"]["kind"] == "config"
    assert "sheet" in ui.save("t.words", path=os.path.join(d, "s.txt"), sheet=True)["error"]["message"]


def test_without_a_path_the_files_come_back_as_blobs(ui):
    r = ui.save("t.loud", format="shout")
    assert r["ok"] and r["paths"] == [] and [f["name"] for f in r["files"]] == ["result.shout"], r
    assert _inline(r["files"][0]["ref"]) == b"PLUM!"
    r = ui.save("t.words")
    assert [f["name"] for f in r["files"]] == ["result-00.txt", "result-01.txt"]
    assert _inline(r["files"][1]["ref"]) == b"b"


def test_what_cant_be_saved_is_refused_as_data(ui):
    d = scratch.mkdtemp("save-refused-")
    for kind, extra, want in [
        ("t.text", {"format": "csv"}, "doesn't save as"),
        ("t.text", {"path": os.path.join(d, "a.wav")}, "doesn't save .wav"),
        ("t.loud", {"path": os.path.join(d, "a.shout"), "options": {"mark": "."}}, "must be one of"),
        ("t.loud", {"path": os.path.join(d, "a.shout"), "options": {"volume": 11}}, "no option"),
        ("t.text", {"port": "nope"}, "no output"),
        ("t.text", {"index": 1}, "one value"),
    ]:
        e = ui.save(kind, **extra)["error"]
        assert e["kind"] == "config" and want in e["message"], (extra, e)
    e = ui.save("t.fails", path=os.path.join(d, "f.txt"))["error"]
    assert e == {"kind": "op", "message": "it broke"}, "the node's own error"
    assert not os.path.exists(os.path.join(d, "f.txt"))
    os.makedirs(os.path.join(d, "taken.txt"))
    assert ui.save("t.text", path=os.path.join(d, "taken.txt"))["ok"] is False
    junk = ui.call("output.save", {"graph": 7, "node": None})["result"]
    assert junk["ok"] is False, "hostile params are a failed save, not a crash"


def test_the_encoders_write_the_rust_sdks_bytes():
    # the same table, quoted the same way as crates/engine/src/save.rs's csv test
    t = {"columns": [{"name": "a", "values": [1, 2.5, None]}, {"name": "b,c", "values": ["x", 'say "hi"', True]}]}
    assert save.csv_bytes(t) == b'a,"b,c"\n1,x\n2.5,"say ""hi"""\n,true\n'
    a = AudioValue(np.array([[0.5, -1.0], [1.5, np.nan]], dtype="<f4"), 8000)
    f = save.wav_bytes(a, False)
    assert f[:4] == b"RIFF" and f[8:16] == b"WAVEfmt " and int.from_bytes(f[20:22], "little") == 3 and len(f) == 44 + 16
    p = save.wav_bytes(a, True)
    assert list(np.frombuffer(p[44:], dtype="<i2")) == [16384, -32767, 32767, 0]
    b = save.npy_bytes("<f4", (2, 3), bytes(24))
    n = int.from_bytes(b[8:10], "little")
    assert b[:8] == b"\x93NUMPY\x01\x00" and (10 + n) % 64 == 0
    assert b[10 : 10 + n].decode().startswith("{'descr': '<f4', 'fortran_order': False, 'shape': (2, 3), }")
    path = os.path.join(scratch.mkdtemp("save-npy-"), "a.npy")
    open(path, "wb").write(save.npy_bytes("<i2", (2, 2), np.arange(4, dtype="<i2").tobytes()))
    assert np.load(path).tolist() == [[0, 1], [2, 3]], "numpy reads it"
    assert save.numbered("a.png", 3, True) == ["a-00.png", "a-01.png", "a-02.png"] and save.numbered("a.png", 101, True)[100] == "a-100.png"


def test_pictures_and_defaults_by_type():
    from io import BytesIO

    from PIL import Image

    grey = np.array([[0, 50, 100], [150, 200, 250]], dtype=np.uint8)
    png = save.png().encode(grey, save.SaveCx({}))
    assert np.asarray(Image.open(BytesIO(png))).tolist() == grey.tolist()
    jpg = save.jpeg().encode(np.zeros((2, 2, 3), dtype=np.float32), save.SaveCx({"quality": 95}))
    assert jpg[:3] == b"\xff\xd8\xff"
    with pytest.raises(NodeError, match=r"\.npy"):
        save.png().encode(np.zeros(5, dtype=np.float32), save.SaveCx({}))
    ids = [f.id for f in save.formats_of(["TABLE", "TEXT", "ARRAY", "AUDIO"], [], [], True)]
    assert ids == ["txt", "csv", "json", "png", "jpeg", "wav", "npy"], "the Rust SDK's order"
    assert save.options(save.jpeg(), {}) == {"quality": 90}
    for bad in [{"quality": 0}, {"quality": 50.5}, {"quality": "best"}, {"nope": 1}]:
        with pytest.raises(NodeError):
            save.options(save.jpeg(), bad)


def _sheet_items():
    """The shared fixture's items, made by the same formula as the Rust SDK's `sheet_items`."""
    a = np.array([[0, 50, 100], [150, 200, 250]], dtype=np.uint8)
    y, x, c = np.mgrid[0:300, 0:100, 0:3]
    b = ((y + 2 * x + 50 * c) % 256).astype(np.uint8)
    yy, xx = np.mgrid[0:10, 0:10]
    cc = np.dstack([xx * 25, yy * 25, np.full((10, 10), 128), (xx + yy) * 12]).astype(np.uint8)
    return [a, b, cc]


def test_a_contact_sheet_has_the_rust_sdks_exact_pixels():
    """Decision 2A: the same sheet, pixel for pixel, as `crates/engine/src/save.rs` makes
    (`crates/engine/tests/fixtures/sheet.json` holds its shape and SHA-256)."""
    import hashlib
    import pathlib

    fixture = json.loads((pathlib.Path(__file__).parents[2] / "crates" / "engine" / "tests" / "fixtures" / "sheet.json").read_text())
    sheet = save.contact_sheet(_sheet_items(), fixture["labels"])
    assert list(sheet.shape) == fixture["shape"]
    assert hashlib.sha256(sheet.tobytes()).hexdigest() == fixture["sha256"]
    with pytest.raises(NodeError, match="item 0:"):
        save.contact_sheet(["text"])
    with pytest.raises(NodeError):
        save.contact_sheet([])


def test_a_list_of_pictures_saves_as_one_contact_sheet_with_the_workflows_name(ui):
    from io import BytesIO

    from PIL import Image

    d = scratch.mkdtemp("save-sheet-")
    info = {"name": "Flow", "hash": "sha256:0123"}
    r = ui.save("t.pics", path=os.path.join(d, "sheet.png"), sheet=True, graph_info=info)
    assert r["ok"] and r["items"] == 3 and r["labels"] == ["a 1", "a 2", "a 3"] and len(r["paths"]) == 1 and r.get("stamped") and not r["recipe"], r
    png = open(os.path.join(d, "sheet.png"), "rb").read()
    img = np.asarray(Image.open(BytesIO(png)).convert("RGB"))
    assert img.shape == (2 * (64 + 15) + 24, 2 * 64 + 24, 3)
    assert img[39, 39].tolist() == [10] * 3 and img[39, 111].tolist() == [200] * 3 and img[126, 39].tolist() == [90] * 3
    assert (img[74:85, 8:72] == 32).all(axis=2).any(), "the first label, drawn in its band"
    assert b"PlumCraft graph" in png
    assert "sheet" in ui.save("t.pics", path=os.path.join(d, "sheet.npy"), sheet=True)["error"]["message"]
    t = ui.save("t.text", path=os.path.join(d, "w.txt"), graph_info=info)
    assert t["ok"] and not t.get("stamped") and open(os.path.join(d, "w.txt"), "rb").read() == b"plum"


def test_the_workflows_name_and_hash_are_the_rust_sdks_bytes():
    """The same chunks and fields as the Rust SDK's `png_with_info`, `wav_with_info`, `svg_with_info`."""
    info = {"name": "Flow é<1>", "hash": "sha256:ab"}
    png = save.png().encode(np.array([[0, 50, 100], [150, 200, 250]], dtype=np.uint8), save.SaveCx({}, "", info))
    at = png.index(b"iTXt")
    n = int.from_bytes(png[at - 4 : at], "big")
    assert png[at + 4 : at + 4 + n] == "PlumCraft graph\0\0\0\0\0Flow é<1>".encode()
    assert b"PlumCraft graph hash\0\0\0\0\0sha256:ab" in png and png.endswith(b"IEND\xae\x42\x60\x82")
    assert save.png_with_info(b"not a png", info) == b"not a png"
    wav = save.wav().encode(AudioValue(np.zeros(3, dtype="<f4"), 8000), save.SaveCx({"bits": "float32"}, "", info))
    assert int.from_bytes(wav[4:8], "little") == len(wav) - 8 and len(wav) % 2 == 0
    k = wav.index(b"LIST")
    assert wav[k + 8 : k + 16] == b"INFOICMT" and wav[k + 20 :].startswith('PlumCraft graph "Flow é<1>", sha256:ab\0'.encode())
    svg = b'<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" data-x="a>b"><rect width="4" height="4"/></svg>'
    assert '"a>b"><metadata>PlumCraft graph "Flow é&lt;1&gt;", sha256:ab</metadata><rect' in save.svg_with_info(svg, info).decode()
    assert save.svg_with_info(b'<svg xmlns="http://www.w3.org/2000/svg"/>', info).endswith(b"</metadata></svg>")
    assert save.txt().encode("plum", save.SaveCx({}, "", info)) == b"plum"


def test_a_persons_save_of_a_non_picture_shape_is_asked_about_first(ui):
    """Decisions 2A and 3A, as the Rust SDK: the question instead of a file; a script's save and
    NumPy are never asked; the rule's edges."""
    d = scratch.mkdtemp("save-question-")
    r = ui.save("t.strip", path=os.path.join(d, "s.png"), ask=True)
    q = r.get("question")
    assert r["ok"] is False and q["shape"] == [4, 1000] and q["offer"] == "npy" and "shorter side" in q["text"], r
    assert r["error"]["kind"] == "config" and not os.path.exists(os.path.join(d, "s.png"))
    assert ui.save("t.strip", path=os.path.join(d, "s.png"))["ok"], "a script's save writes it"
    assert ui.save("t.strip", path=os.path.join(d, "s.npy"), ask=True)["ok"], "NumPy is never asked"
    assert "output.save.question" in ui.caps
    z = lambda *shape: np.zeros(shape, dtype=np.uint8)  # noqa: E731
    assert save.picture_shape(z(64, 64)) is None and save.picture_shape(z(64, 1024, 4)) is None
    assert "16 times" in save.picture_shape(z(64, 1025))
    assert save.picture_shape(z(63, 63)) is not None and "5 channels" in save.picture_shape(z(64, 64, 5))
    assert "1 dimensions" in save.picture_shape(z(4096))
    from plumcraft_engine import ArrayValue

    assert "table" in save.picture_shape(ArrayValue(z(100, 3), table=True))
