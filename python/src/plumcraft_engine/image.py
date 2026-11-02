"""IMAGE, a core type (`plumcraft_proto::catalogue::core_types::IMAGE`; maintainer, 2026-10-30:
"yes core data types"): an encoded picture as an `md5:raw:` blob whose media names its format.
Every engine that reads IMAGE reads PNG, and makes PNG unless asked for another format.

`image(data)` makes the value a node returns from an `Image` output: its format read from its
first bytes (PNG, JPEG, TIFF, GIF, WebP), its size from a PNG's header (0 for the others, as
unknown). Bytes that are none of these are a `config` error. Pass `pixels`, the array it encodes,
and the node gets a thumbnail.
"""

from __future__ import annotations

import struct
from typing import Any

import numpy as np

from . import Blob, NodeError, arrays
from .data import raw

# The largest picture made (`plumcraft_store::MAX_READ`'s order).
MAX_BYTES = 1 << 30

_SIGNATURES = (
    (b"\x89PNG\r\n\x1a\n", "image/png"),
    (b"\xff\xd8\xff", "image/jpeg"),
    (b"II*\x00", "image/tiff"),
    (b"MM\x00*", "image/tiff"),
    (b"GIF87a", "image/gif"),
    (b"GIF89a", "image/gif"),
)


def sniff(data: bytes) -> str | None:
    """The media type of an encoded picture, from its first bytes; None when it isn't one."""
    for sig, media in _SIGNATURES:
        if data.startswith(sig):
            return media
    if data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return "image/webp"
    return None


def png_size(data: bytes) -> tuple[int, int]:
    """A PNG's width and height from its IHDR chunk, or (0, 0)."""
    if len(data) < 24 or data[12:16] != b"IHDR":
        return 0, 0
    w, h = struct.unpack(">II", data[16:24])
    return int(w), int(h)


def image(data: bytes, pixels: Any = None) -> Blob:
    """An IMAGE value (see the module docs)."""
    data = bytes(data)
    if len(data) > MAX_BYTES:
        raise NodeError(NodeError.CONFIG, f"not a picture: {len(data) >> 20} MB is too large")
    media = sniff(data)
    if media is None:
        raise NodeError(NodeError.CONFIG, "not a picture: PNG, JPEG, TIFF, GIF or WebP bytes")
    w, h = png_size(data) if media == "image/png" else (0, 0)
    meta = {"media": media, "size": len(data), "form": {"kind": "image", "width": w, "height": h}}
    return Blob(handle=raw(data), meta=meta, data=data, thumb=None if pixels is None else arrays.thumb(np.asarray(pixels), 160))
