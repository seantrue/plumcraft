"""Weather: a PlumCraft engine. Every method marked `@node` is a node: its annotations are its ports,
its docstring its description. README.md says how to add one; the PlumCraft SDK's `python/README.md`
has the full list of annotations."""

import csv
import io
import json
import re
import time
from typing import Annotated, Literal

import numpy as np
from PIL import Image as Pil
from plumcraft_engine import (
    Array,
    ArrayValue,
    Audio,
    AudioValue,
    Blob,
    ConfigError,
    Cx,
    Engine,
    Float,
    Image,
    Json,
    List,
    Node,
    NodeError,
    OpError,
    Port,
    Range,
    String,
    Svg,
    Table,
    Text,
    image,
    node,
    svg,
)

__version__ = "0.1.0"

HEX_COLOR = re.compile(r"#[0-9a-fA-F]{6}")


def png(pixels: np.ndarray) -> bytes:
    """H × W or H × W × 3 or 4, 8 bits a channel, as PNG bytes."""
    out = io.BytesIO()
    Pil.fromarray(pixels).save(out, format="PNG")
    return out.getvalue()


def decoded(picture: Blob) -> Pil.Image:
    """An IMAGE's pixels. Bytes the library can't read are an op error: a failure on this input."""
    try:
        return Pil.open(io.BytesIO(picture.data))
    except OSError as e:
        raise OpError(f"can't read this {picture.meta.get('media')}: {e}") from None


class Split(Node):
    """Text to a list of words. Written in the low-level layer because `list` ports are what a
    signature can't say yet: a `Node` subclass with its NodeDef as a dict, and `run(inputs, cx)`."""

    definition = {
        "id": "weather.split",
        "display": "Split",
        "category": ["Weather"],
        "description": "Text as a list of its words. Wired into an ordinary input, the next node runs once per word. The inverse of Join List.",
        "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}, {"name": "separator", "type": "STRING", "default": " "}],
        "outputs": [{"name": "items", "type": "TEXT", "list": True, "socket": "socket"}],
    }

    def run(self, inputs: dict, cx: Cx) -> dict:
        separator = inputs.get("separator")
        if not isinstance(separator, str) or not separator:
            raise ConfigError("a separator is some text")
        return {"items": List(inputs["text"].split(separator))}


class JoinList(Node):
    """A list of words to text. A `list` input receives the whole list at once."""

    definition = {
        "id": "weather.join_list",
        "display": "Join List",
        "category": ["Weather"],
        "description": "A list of texts joined into one. The inverse of Split.",
        "inputs": [{"name": "items", "type": "TEXT", "list": True, "socket": "socket"}, {"name": "separator", "type": "STRING", "default": " "}],
        "outputs": [{"name": "text", "type": "TEXT"}],
        "flags": {"input_list": True},
    }

    def run(self, inputs: dict, cx: Cx) -> dict:
        items = inputs["items"]
        separator = inputs.get("separator")
        if not isinstance(separator, str):
            raise ConfigError("a separator is some text")
        return {"text": separator.join(str(i) for i in (items.items if isinstance(items, List) else [items]))}


class WeatherEngine(Engine):
    id = "weather"
    name = "Weather"
    version = __version__
    category = "Weather"
    family = "weather"
    # Their versions join the memo salt: an upgrade never serves an old result.
    dependencies = ("pillow",)
    # Every building block (Vary, sequences, files), as ANY ports.
    building_blocks = True

    def __init__(self, info: dict | None = None):
        super().__init__(info)
        self.with_node(Split()).with_node(JoinList())

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

    # ---------- tables ----------

    @node("Sample Table")
    def sample_table(self, rows: Annotated[int, Range(1, 1000, invalid="clamp")] = 5) -> Table:
        """A source: a small table. A table is columns of equal length: `{"columns": [{"name", "type", "values"}]}`."""
        names = ["corgi", "heron", "otter", "wren", "vole"]
        return {
            "columns": [
                {"name": "name", "type": "STRING", "values": [names[i % len(names)] for i in range(rows)]},
                {"name": "count", "type": "FLOAT", "values": [float((i * 7) % 11) for i in range(rows)]},
            ]
        }

    @node("Sort Rows")
    def sort_rows(self, table: Table, column: str = "count", descending: bool = False) -> Table:
        """The table's rows sorted by a column. A column that isn't there is a config error."""
        columns = table["columns"]
        key = next((c["values"] for c in columns if c["name"] == column), None)
        if key is None:
            raise ConfigError(f"no column {column!r}; the columns are {', '.join(c['name'] for c in columns)}")
        order = sorted(range(len(key)), key=lambda i: key[i], reverse=descending)
        return {"columns": [c | {"values": [c["values"][i] for i in order]} for c in columns]}

    # ---------- arrays ----------

    @node("Ramp")
    def ramp(self, width: Annotated[int, Range(1, 4096, invalid="clamp")] = 64, height: Annotated[int, Range(1, 4096, invalid="clamp")] = 64) -> Array:
        """A source: a colour gradient, H × W × 3, 0-1. `ArrayValue` says what the array is: its colour space and channels."""
        x, y = np.meshgrid(np.linspace(0, 1, width, dtype="f4"), np.linspace(0, 1, height, dtype="f4"))
        return ArrayValue(np.stack([x, y, 1 - x], axis=2), space="RGB", names=("red", "green", "blue"))

    @node("Normalize")
    def normalize(self, array: Array) -> Array:
        """The array scaled to 0-1. A flat array becomes zeros. Arithmetic gives a plain array: the space no longer holds."""
        low, high = float(np.nanmin(array)), float(np.nanmax(array))
        return (np.asarray(array, dtype="f4") - low) / (high - low) if high > low else np.zeros_like(array, dtype="f4")

    # ---------- SVG ----------

    @node("Shapes")
    def shapes(self, count: Annotated[int, Range(1, 50, invalid="clamp")] = 5, color: str = "#6a3d9a") -> Svg:
        """A source: circles in a row. `svg()` checks the document and reads its size; the editor draws it."""
        if not HEX_COLOR.fullmatch(color):
            raise ConfigError(f"{color!r} isn't a colour like #6a3d9a")
        circles = "".join(f'<circle cx="{20 + 40 * i}" cy="20" r="16" fill="{color}"/>' for i in range(count))
        return svg(f'<svg xmlns="http://www.w3.org/2000/svg" width="{40 * count}" height="40">{circles}</svg>')

    @node("Recolor")
    def recolor(self, drawing: Svg, color: str = "#2ca02c") -> Svg:
        """Every fill in the SVG set to one colour. An SVG arrives as a `Blob`: its bytes are in `.data`."""
        if not HEX_COLOR.fullmatch(color):
            raise ConfigError(f"{color!r} isn't a colour like #2ca02c")
        return svg(re.sub(r'fill="[^"]*"', f'fill="{color}"', drawing.data.decode("utf-8")))

    # ---------- pictures ----------

    @node("Checkerboard")
    def checkerboard(self, size: Annotated[int, Range(8, 1024, invalid="clamp")] = 64, cells: Annotated[int, Range(1, 32, invalid="clamp")] = 8) -> Image:
        """A source: a PNG. `image()` reads the format from the bytes; an IMAGE is an encoded picture, PNG unless asked otherwise."""
        y, x = np.indices((size, size)) * cells // size
        return image(png(((x + y) % 2 * 255).astype("u1")))

    @node("Image Size")
    def image_size(self, picture: Image) -> String:
        """A picture's width and height. A picture arrives as a `Blob`: its bytes are in `.data`."""
        width, height = decoded(picture).size
        return f"{width}x{height}"

    # ---------- audio ----------

    @node("Tone")
    def tone(
        self,
        hz: Annotated[float, Range(20, 20000, invalid="clamp")] = 440.0,
        seconds: Annotated[float, Range(0.01, 10, invalid="clamp")] = 0.25,
        rate: Annotated[int, Range(8000, 192000, invalid="clamp")] = 44100,
    ) -> Audio:
        """A source: a sine wave. `AudioValue` is samples, frames × channels, float32, and a sample rate."""
        t = np.arange(int(seconds * rate)) / rate
        return AudioValue(0.5 * np.sin(2 * np.pi * hz * t), rate)

    @node("Gain")
    def gain(self, audio: Audio, db: Annotated[float, Range(-60, 24, invalid="clamp")] = -6.0) -> Audio:
        """Audio louder or quieter, in decibels."""
        return AudioValue(audio.samples * 10 ** (db / 20), audio.sample_rate)

    # ---------- JSON ----------

    @node("Settings")
    def settings(self, name: str = "corgi", size: Annotated[int, Range(0, 100, invalid="clamp")] = 3) -> Json:
        """A source: any JSON value. It stays JSON on the wire; the editor shows it as YAML."""
        return {"name": name, "size": size, "tags": ["small", "loud"], "owner": {"present": True}}

    @node("Get Key")
    def get_key(self, value: Json, key: str = "name") -> Json:
        """One key of a JSON object. A key that isn't there is a config error."""
        if not isinstance(value, dict) or key not in value:
            raise ConfigError(f"no key {key!r} in this value")
        return value[key]

    @node("Table to CSV")
    def table_to_csv(self, table: Table) -> Text:
        """A table as CSV text, a header line first. The inverse of CSV to Table."""
        columns = table["columns"]
        out = io.StringIO()
        writer = csv.writer(out, lineterminator="\n")
        writer.writerow(c["name"] for c in columns)
        writer.writerows(zip(*(c["values"] for c in columns), strict=True))
        return out.getvalue()

    @node("CSV to Table")
    def csv_to_table(self, text: Text) -> Table:
        """CSV text as a table: the first line names the columns, and a column of numbers is FLOAT. The inverse of Table to CSV."""
        rows = list(csv.reader(io.StringIO(text)))
        if not rows:
            raise ConfigError("the CSV is empty")
        columns = []
        for i, name in enumerate(rows[0]):
            values = [r[i] if i < len(r) else "" for r in rows[1:]]
            try:
                columns.append({"name": name, "type": "FLOAT", "values": [float(v) for v in values]})
            except ValueError:
                columns.append({"name": name, "type": "STRING", "values": values})
        return {"columns": columns}

    @node("Table to Array")
    def table_to_array(self, table: Table) -> Array:
        """A table's FLOAT and INT columns as rows × columns. Text columns are left out. The inverse of Array to Table."""
        numeric = [c for c in table["columns"] if c.get("type") in ("FLOAT", "INT")]
        if not numeric:
            raise ConfigError("the table has no numeric column")
        data = np.array([c["values"] for c in numeric], dtype="f8").T
        return ArrayValue(data, names=[c["name"] for c in numeric], table=True)

    @node("Array to Table")
    def array_to_table(self, array: Array) -> Table:
        """A rows × columns array as a table of FLOAT columns. The inverse of Table to Array."""
        data = np.asarray(array)
        if data.ndim != 2:
            raise ConfigError(f"an array of rows and columns, not {'×'.join(str(n) for n in data.shape)}")
        names = list(getattr(array, "names", ())) or [f"c{i + 1}" for i in range(data.shape[1])]
        return {"columns": [{"name": n, "type": "FLOAT", "values": data[:, i].tolist()} for i, n in enumerate(names)]}

    @node("SVG to Text")
    def svg_to_text(self, drawing: Svg) -> Text:
        """An SVG's source. The inverse of Text to SVG."""
        return drawing.data.decode("utf-8")

    @node("Text to SVG")
    def text_to_svg(self, text: Text) -> Svg:
        """Text as an SVG; text that isn't one is a config error. The inverse of SVG to Text."""
        return svg(text)

    @node("Image to Array")
    def image_to_array(self, picture: Image) -> Array:
        """A picture's pixels, H × W × 3, 0-1. The inverse of Array to Image."""
        pixels = np.asarray(decoded(picture).convert("RGB"), dtype="f4") / 255
        return ArrayValue(pixels, space="RGB", names=("red", "green", "blue"))

    @node("Array to Image")
    def array_to_image(self, array: Array) -> Image:
        """A picture (H × W, or H × W × 3 or 4, 0-1) as a PNG, clipped and rounded to 8 bits. The inverse of Image to Array."""
        data = np.asarray(array)
        if data.ndim == 1 or data.ndim > 3 or (data.ndim == 3 and data.shape[2] not in (3, 4)):
            raise ConfigError(f"a picture, not {'×'.join(str(n) for n in data.shape)}")
        return image(png((np.nan_to_num(np.clip(data, 0, 1)) * 255 + 0.5).astype("u1")))

    @node("Audio to Array")
    def audio_to_array(self, audio: Audio) -> Array:
        """Audio's samples, frames × channels. An array has no sample rate. The inverse of Array to Audio."""
        return audio.samples

    @node("Array to Audio")
    def array_to_audio(self, array: Array, rate: Annotated[int, Range(8000, 192000, invalid="clamp")] = 44100) -> Audio:
        """Samples, frames or frames × channels, as audio at a sample rate. The inverse of Audio to Array."""
        data = np.asarray(array)
        if data.ndim not in (1, 2) or data.size == 0:
            raise ConfigError(f"samples, not {'×'.join(str(n) for n in data.shape)}")
        return AudioValue(data, rate)

    @node("JSON to Text")
    def json_to_text(self, value: Json) -> Text:
        """A JSON value as text. The inverse of Text to JSON."""
        return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False)

    @node("Text to JSON")
    def text_to_json(self, text: Text) -> Json:
        """Text as a JSON value; text that isn't JSON is a config error. The inverse of JSON to Text."""
        try:
            return json.loads(text)
        except ValueError as e:
            raise ConfigError(f"not JSON: {e}") from None

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
