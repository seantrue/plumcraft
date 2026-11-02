"""The text engine in Python: the same nodes as the Rust `plumcraft-text` crate, with the same
catalogue and the same results, so the UI cannot tell them apart except by `engine.salt` and the
`publisher` on status records. The conformance suite runs every case against both.
"""

from __future__ import annotations

import os
import re
import time
from typing import Annotated

import numpy as np

from plumcraft_engine import Array, ConfigError, Cx, Engine, File, Float, Int, NodeError, OpError, Range, String, Table, Text, node

__version__ = "0.1.0"

# The largest file `text.load` reads, and the longest text `text.repeat` makes.
MAX_LOAD = 64 << 20
MAX_REPEAT = 64 << 20

# Unicode White_Space, as Rust's `split_whitespace` uses it (Python's `str.split` also splits on
# U+001C..U+001F, which would count words differently).
_WHITESPACE = re.compile("[\t\n\x0b\x0c\r \x85\xa0  -     　]+")


class TextEngine(Engine):
    id = "text"
    name = "Text"
    version = __version__
    salt = f"py:plumcraft-text+{__version__}"
    category = "Text"
    family = "text"
    wires = (Text, String, Int, Float, Table, Array)
    # Every building block (Vary, sequences, files), as the Rust engine's `.with_building_blocks()`.
    building_blocks = True

    @node("Text")
    def literal(self, text: str = "") -> Text:
        return text

    @node("Upper case")
    def upper(self, text: Text) -> Text:
        return text.upper()

    @node("Lower case")
    def lower(self, text: Text) -> Text:
        return text.lower()

    @node("Reverse")
    def reverse(self, text: Text) -> Text:
        return text[::-1]

    @node("Replace")
    def replace(self, text: Text, find: str = "", with_: str = "") -> Text:
        if not find:
            raise ConfigError("find is empty")
        return text.replace(find, with_)

    @node("Join")
    def join(self, a: Text, b: Text, sep: str = " ") -> Text:
        return f"{a}{sep}{b}"

    @node("Word count", output="count")
    def words(self, text: Text) -> int:
        return len([w for w in _WHITESPACE.split(text) if w])

    @node("Fail")
    def fail(self, text: Text) -> Text:
        raise OpError("this node always fails")

    # Passes text through after `seconds`, reporting progress and stopping early on cancel.
    @node("Slow", category=("Text", "Testing"))
    def slow(self, text: Text, seconds: Annotated[float, Range(0, 600, invalid="clamp")] = 1.0, *, cx: Cx) -> Text:
        start = time.monotonic()
        while time.monotonic() - start < seconds:
            if cx.cancelled():
                raise NodeError.cancelled()
            done = (time.monotonic() - start) / max(seconds, 1e-12)
            cx.progress(done, f"{done * 100:.0f}%")
            time.sleep(0.01)
        return text

    # As the Rust engine: anything but a non-negative integer repeats twice, a million at most.
    @node("Repeat")
    def repeat(self, text: Text, times: Annotated[int, Range(0, 1e6, invalid="pass")] = 2) -> Text:
        times = min(times, 1_000_000) if isinstance(times, int) and not isinstance(times, bool) and times >= 0 else 2
        if times * len(text.encode("utf-8")) > MAX_REPEAT:
            raise ConfigError(f"the result would exceed {MAX_REPEAT >> 20} MiB")
        return text * times

    # The Latin letters a-z, case-folded, in alphabetical order (as the Rust engine: Rust's
    # `is_alphabetic` and Python's `isalpha` disagree on parts of Unicode).
    @node("Letter counts (a–z)")
    def letters(self, text: Text) -> Table:
        counts: dict[str, int] = {}
        for c in text:
            if c.isascii() and c.isalpha():
                counts[c.lower()] = counts.get(c.lower(), 0) + 1
        keys = sorted(counts)
        return {"columns": [{"name": "letter", "type": "STRING", "values": keys}, {"name": "count", "type": "INT", "values": [counts[k] for k in keys]}]}

    @node("Code points", output="codes")
    def codes(self, text: Text) -> Array:
        return np.array([ord(c) for c in text], dtype="<u4")

    @node("Load text")
    def load(self, path: Annotated[str, File("open")] = "") -> Text:
        if not path:
            raise ConfigError("path is empty")
        try:
            size = os.path.getsize(path)
        except OSError as e:
            raise ConfigError(f"{path}: {e.strerror}") from None
        if size > MAX_LOAD:
            raise ConfigError(f"{path}: larger than {MAX_LOAD >> 20} MiB")
        try:
            with open(path, "rb") as f:
                raw = f.read()
        except OSError as e:
            raise OpError(f"{path}: {e.strerror}") from None
        try:
            return raw.decode("utf-8")
        except UnicodeDecodeError:
            raise OpError(f"{path}: not UTF-8 text") from None


def engine() -> TextEngine:
    """The engine, as a host's factory (kept for callers of the low-level API)."""
    return TextEngine()
