"""Weather: a PlumCraft engine. Every method marked `@node` is a node: its annotations are its ports,
its docstring its description. README.md says how to add one; the PlumCraft SDK's `python/README.md`
has the full list of annotations."""

import time
from typing import Annotated, Literal

from plumcraft_engine import ConfigError, Cx, Engine, Float, NodeError, OpError, Port, Range, String, Text, node

__version__ = "0.1.0"


class WeatherEngine(Engine):
    id = "weather"
    name = "Weather"
    version = __version__
    category = "Weather"
    family = "weather"

    # ---------- numbers ----------

    @node("Add")
    def add(self, a: Annotated[float, Range(-1e9, 1e9)] = 1.0, b: Annotated[float, Range(-1e9, 1e9), Port(label="Plus", tooltip="Added to a")] = 2.0) -> Float:
        """Add two numbers. Widgets that can also be wired, with a range the engine enforces."""
        return a + b

    @node("Format Number")
    def format_number(self, value: Annotated[float, Range(-1e9, 1e9)] = 1.5, decimals: Annotated[int, Range(0, 12, invalid="clamp")] = 2) -> String:
        """A number as text. The inverse of Parse Number. `invalid="clamp"` brings decimals into range."""
        return f"{value:.{decimals}f}"

    @node("Parse Number")
    def parse_number(self, string: str = "1.5") -> Float:
        """Text as a number. The inverse of Format Number. Text that isn't one is a config error: the user can fix it."""
        try:
            return float(string)
        except ValueError:
            raise ConfigError(f"{string!r} isn't a number") from None

    # ---------- text ----------

    @node("Text")
    def text(self, text: str = "a corgi naps") -> Text:
        """A source: the text typed into its widget."""
        return text

    @node("Shout")
    def shout(
        self,
        text: Text,
        marks: Annotated[int, Range(0, 5, invalid="clamp"), Port(tooltip="How many exclamation marks")] = 2,
        case: Literal["upper", "title"] = "upper",
    ) -> Text:
        """Loud text. A socket, a clamped widget and a choice; empty text is a config error."""
        if not text.strip():
            raise ConfigError("there is nothing to shout")
        return (text.upper() if case == "upper" else text.title()) + "!" * marks

    @node("Text to Hex")
    def text_to_hex(self, text: Text) -> String:
        """Text as the hex of its UTF-8 bytes. The inverse of Hex to Text."""
        return text.encode("utf-8").hex()

    @node("Hex to Text")
    def hex_to_text(self, string: str = "636f726769") -> Text:
        """Hex back to text. The inverse of Text to Hex. Hex that isn't UTF-8 is an op error: a failure on this input."""
        try:
            return bytes.fromhex(string).decode("utf-8")
        except ValueError as e:
            raise OpError(f"not the hex of UTF-8 text: {e}") from None

    # ---------- testing ----------

    @node("Slow", category=("Weather", "Testing"))
    def slow(self, text: Text, seconds: Annotated[float, Range(0, 600, invalid="clamp")] = 0.05, *, cx: Cx) -> Text:
        """Passes its input through after `seconds`, reporting progress and stopping on cancel. How a long node behaves."""
        start = time.monotonic()
        while time.monotonic() - start < seconds:
            if cx.cancelled():
                raise NodeError.cancelled()
            cx.progress((time.monotonic() - start) / max(seconds, 1e-9), "waiting")
            time.sleep(0.01)
        return text
