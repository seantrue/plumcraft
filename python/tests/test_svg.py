"""SVG, a core type, in the Python SDK: `Svg` declares exactly the Rust SDK's wire type, `svg()`
makes the value (checked, sized), and an engine's SVG output crosses as an `md5:raw:` blob of
media `image/svg+xml`."""

import base64
import time

import pytest

from plumcraft_engine import Blob, Engine, Image, NodeError, Svg, Text, image, node, svg
from plumcraft_engine.data import raw as _raw
from plumcraft_engine.host import Host

BOX = '<svg xmlns="http://www.w3.org/2000/svg" width="30px" height="20"><rect width="30" height="20"/></svg>'


class Drawn(Engine):
    id = "drawn"
    name = "Drawn"
    version = "1"

    @node
    def box(self, side: int = 10) -> Svg:
        return svg(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {side} {side}"><rect width="{side}" height="{side}"/></svg>')


def test_svg_values_are_checked_and_sized():
    b = svg(BOX)
    assert b.meta == {"media": "image/svg+xml", "size": len(BOX), "form": {"kind": "vector", "width": 30.0, "height": 20.0}}
    assert b.handle.startswith("md5:raw:") and b.data == BOX.encode()
    assert svg(b'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0,0,8,4"/>').meta["form"]["width"] == 8.0
    assert svg('<svg xmlns="http://www.w3.org/2000/svg" width="50%"/>').meta["form"]["width"] == 0.0, "unknown units are unknown"
    for bad in ("<html/>", "not xml", ""):
        with pytest.raises(NodeError) as e:
            svg(bad)
        assert e.value.kind == "config" and "not an SVG" in str(e.value)


def test_the_svg_type_is_the_core_one():
    [w] = [w for w in Drawn().wire_types if w["id"] == "SVG"]
    # plumcraft_proto::catalogue::core_types::svg()
    assert w == {"id": "SVG", "media": "vector", "color": "#16a34a", "thumb": True}


def test_an_svg_output_crosses_as_a_blob():
    h = Host(Drawn)
    try:
        assert h.wait_ready(5)["state"] == "connected"
        graph = {"nodes": {"b": {"kind": "drawn.box", "values": {"side": 12}}}, "wires": []}
        h.run({"v": 2, "op": "run", "run_id": "r", "generation": 1, "graph": {"kind": "inline", "graph": graph}, "targets": ["b"]})
        deadline, since = time.monotonic() + 10, 0
        while time.monotonic() < deadline:
            p = h.poll(since, 200, "r")
            since = p["last_seq"]
            reply = next((e["body"] for e in p["events"] if e["kind"] == "run.reply"), None)
            if reply:
                break
        out = reply["nodes"]["b"]["outputs"]["svg"]
        meta = out["ref"]["meta"]
        assert out["kind"] == "blob" and meta["media"] == "image/svg+xml" and meta["form"] == {"kind": "vector", "width": 12.0, "height": 12.0}, out
    finally:
        h.shutdown()


# ---------- SVG and IMAGE as inputs ----------


def _png(w: int, h: int) -> bytes:
    """A w × h grey PNG, made with the standard library."""
    import struct
    import zlib

    def chunk(kind: bytes, body: bytes) -> bytes:
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    rows = b"".join(b"\x00" + b"\x80" * w for _ in range(h))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


class Takes(Engine):
    id = "takes"
    name = "Takes"
    version = "1"

    @node
    def box(self, side: int = 10) -> Svg:
        return svg(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {side} {side}"/>')

    @node
    def grey(self, w: int = 3, h: int = 2) -> Image:
        return image(_png(w, h))

    @node
    def svg_size(self, s: Svg) -> Text:
        f = s.meta["form"]
        return f"{s.meta['media']} {f['width']:g}x{f['height']:g} {len(s.data)} bytes"

    @node
    def image_size(self, p: Image) -> Text:
        f = p.meta["form"]
        return f"{p.meta['media']} {f['width']}x{f['height']} {p.data[:4]!r}"


def test_image_values_are_checked_and_sized():
    b = image(_png(5, 4))
    assert b.meta == {"media": "image/png", "size": len(b.data), "form": {"kind": "image", "width": 5, "height": 4}}
    assert image(b"\xff\xd8\xff\xe0" + bytes(20)).meta["media"] == "image/jpeg"
    with pytest.raises(NodeError) as e:
        image(b"<svg/>")
    assert e.value.kind == "config"


def _reply(h: Host, run_id: str, req: dict) -> dict:
    h.run({"v": 2, "op": "run", "run_id": run_id, "generation": 1, **req})
    deadline, since = time.monotonic() + 10, 0
    while time.monotonic() < deadline:
        p = h.poll(since, 200, run_id)
        since = p["last_seq"]
        for e in p["events"]:
            if e["kind"] == "run.reply":
                return e["body"]
    raise AssertionError(f"no reply for {run_id}")


def test_svg_and_image_inputs_receive_blobs_wired_or_bound():
    h = Host(Takes)
    try:
        assert h.wait_ready(5)["state"] == "connected"
        wire = lambda a, ap, b, bp: {"from": {"node": a, "port": ap}, "to": {"node": b, "port": bp}}  # noqa: E731
        wires = [wire("b", "svg", "s", "s"), wire("g", "image", "i", "p")]
        nodes = {"b": {"kind": "takes.box", "values": {"side": 7}}, "s": {"kind": "takes.svg_size"}}
        nodes |= {"g": {"kind": "takes.grey"}, "i": {"kind": "takes.image_size"}}
        r = _reply(h, "w", {"graph": {"kind": "inline", "graph": {"nodes": nodes, "wires": wires}}, "targets": ["s", "i"]})
        assert r["nodes"]["s"]["outputs"]["text"]["value"].startswith("image/svg+xml 7x7 ")
        assert r["nodes"]["i"]["outputs"]["text"]["value"] == "image/png 3x2 b'\\x89PNG'"
        # bound, as another engine's SVG and PNG arrive (inline here)
        doc, png = svg(BOX), image(_png(4, 4))
        ref = lambda b: {"handle": b.handle, "meta": b.meta, "at": [{"kind": "inline", "b64": base64.b64encode(b.data).decode()}]}  # noqa: E731
        graph = {"nodes": {"s": {"kind": "takes.svg_size"}, "i": {"kind": "takes.image_size"}}, "wires": []}
        inputs = {"s.s": {"kind": "blob", "ref": ref(doc)}, "i.p": {"kind": "blob", "ref": ref(png)}}
        r = _reply(h, "b", {"graph": {"kind": "inline", "graph": graph}, "targets": ["s", "i"], "inputs": inputs})
        assert r["nodes"]["s"]["outputs"]["text"]["value"] == f"image/svg+xml 30x20 {len(BOX)} bytes", r["nodes"]["s"]
        assert r["nodes"]["i"]["outputs"]["text"]["value"] == "image/png 4x4 b'\\x89PNG'", r["nodes"]["i"]
    finally:
        h.shutdown()


class Thumbs(Takes):
    id = "thumbs"

    def picture_thumb(self, blob):
        import numpy as np

        if blob.meta["size"] == 0 or blob.data[:4] != b"\x89PNG":
            raise ValueError("not one of mine")
        return np.zeros((2, 2, 4), "u1")

    @node
    def bad(self) -> Image:
        return Blob(handle=_raw(b"nope"), meta={"media": "image/png", "size": 4, "form": {"kind": "image", "width": 0, "height": 0}}, data=b"nope")


def _thumb_of(cls, kind: str) -> dict | None:
    h = Host(cls)
    try:
        assert h.wait_ready(5)["state"] == "connected"
        _reply(h, "t", {"graph": {"kind": "inline", "graph": {"nodes": {"n": {"kind": kind}}, "wires": []}}, "targets": ["n"]})
        events = h.poll(0, 0, "t")["events"]
        return next((e["body"]["thumb"] for e in events if e["kind"] == "node.status" and e["body"].get("thumb")), None)
    finally:
        h.shutdown()


def test_an_engine_can_picture_a_file_and_a_failure_is_no_thumbnail():
    assert _thumb_of(Takes, "takes.grey") is None, "the SDK decodes no pictures"
    assert _thumb_of(Thumbs, "takes.grey")["kind"] == "rgba8"
    assert _thumb_of(Thumbs, "thumbs.bad") is None, "a raising engine loses the thumbnail, not the node"


def test_an_image_made_with_its_pixels_has_a_thumbnail():
    import numpy as np

    b = image(_png(4, 4), pixels=np.zeros((8, 8, 3), "f4"))
    assert b.thumb.shape == (8, 8, 4) and b.thumb.dtype == np.uint8
    assert image(_png(4, 4)).thumb is None, "an engine needn't send one"
