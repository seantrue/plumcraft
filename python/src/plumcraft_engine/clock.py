"""The clock a node's own work is measured on (spec 14.5: whether a result is worth persisting is
weighed against that work), as the Rust crate's `plumcraft_engine::clock`.

An engine uses the real one. A test gives a virtual one (`Engine.with_clock(VirtualClock())`), on
which a node's work is exactly the time it sleeps on that clock, so what the executor decides
doesn't depend on how fast the machine is (B49). Times are seconds, as `time.monotonic` gives.
"""

from __future__ import annotations

import threading
import time
from typing import Protocol


class Clock(Protocol):
    """Now and sleep, for measuring a node's work."""

    def now(self) -> float:
        """Seconds since some fixed moment of this clock's."""
        ...

    def sleep(self, seconds: float) -> None:
        """Wait `seconds` on this clock."""
        ...


class RealClock:
    """The machine's monotonic clock."""

    def now(self) -> float:
        return time.monotonic()

    def sleep(self, seconds: float) -> None:
        time.sleep(max(0.0, seconds))


class VirtualClock:
    """A clock that moves only when something sleeps on it (or `advance`): what a test measures
    work on. Safe to share between threads."""

    def __init__(self) -> None:
        self._now = 0.0
        self._lock = threading.Lock()

    def advance(self, seconds: float) -> None:
        """Move it on by `seconds`, sleeping no real time."""
        with self._lock:
            self._now += max(0.0, seconds)

    def now(self) -> float:
        with self._lock:
            return self._now

    def sleep(self, seconds: float) -> None:
        self.advance(seconds)


__all__ = ["Clock", "RealClock", "VirtualClock"]
