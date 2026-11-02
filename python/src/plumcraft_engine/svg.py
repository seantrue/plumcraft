"""SVG, a core type (`plumcraft_proto::catalogue::core_types::SVG`; maintainer, 2026-10-30: "Make SVG
first class"): an SVG document, UTF-8, crossing as an `md5:raw:` blob of media `image/svg+xml`.

`svg(text)` makes the value a node returns from an `Svg` output: checked to be an SVG document
(its root element is `svg`), its size read from `width` and `height` (or the `viewBox`). The
editor draws SVG itself, so a Python engine sends no picture of it (the Rust SDK, which has a
rasteriser, makes a node thumbnail too).
"""

from __future__ import annotations

import re
import xml.etree.ElementTree as ET

from . import Blob, NodeError
from .data import raw

MEDIA = "image/svg+xml"
# The largest SVG made (`plumcraft_svg::MAX_SVG_BYTES`).
MAX_BYTES = 64 << 20

_LENGTH = re.compile(r"^\s*([0-9.eE+-]+)\s*(px)?\s*$")


def _length(v: str | None) -> float:
    """A plain or `px` length; anything else (percentages, other units) is 0, as unknown."""
    m = _LENGTH.match(v or "")
    try:
        return float(m.group(1)) if m else 0.0
    except ValueError:
        return 0.0


def size(root: ET.Element) -> tuple[float, float]:
    """An SVG's width and height: its attributes, else its viewBox's, else 0."""
    w, h = _length(root.get("width")), _length(root.get("height"))
    if w > 0 and h > 0:
        return w, h
    box = (root.get("viewBox") or "").replace(",", " ").split()
    if len(box) == 4:
        try:
            return float(box[2]), float(box[3])
        except ValueError:
            pass
    return 0.0, 0.0


def svg(text: str | bytes) -> Blob:
    """An SVG value (see the module docs); not an SVG document is a `config` error."""
    data = text.encode("utf-8") if isinstance(text, str) else bytes(text)
    if len(data) > MAX_BYTES:
        raise NodeError(NodeError.CONFIG, f"not an SVG: {len(data) >> 20} MB is too large")
    try:
        root = ET.fromstring(data)
    except ET.ParseError as e:
        raise NodeError(NodeError.CONFIG, f"not an SVG: {e}") from None
    if root.tag.rsplit("}", 1)[-1] != "svg":
        raise NodeError(NodeError.CONFIG, f"not an SVG: its root is <{root.tag.rsplit('}', 1)[-1]}>")
    w, h = size(root)
    return Blob(
        handle=raw(data),
        meta={"media": MEDIA, "size": len(data), "form": {"kind": "vector", "width": w, "height": h}},
        data=data,
    )
