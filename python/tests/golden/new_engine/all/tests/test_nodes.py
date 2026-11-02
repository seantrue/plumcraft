"""Each node called as a plain method: no host, no PlumCraft, quick to run and to debug."""

import numpy as np
import pytest
from plumcraft_engine import ConfigError, OpError

from weather_engine import JoinList, Split


def test_add(engine):
    assert engine.add(1.0, 2.5) == 3.5


def test_format_number(engine):
    assert engine.format_number(1.5, 3) == "1.500"


def test_parse_number(engine):
    assert engine.parse_number("2.25") == 2.25
    with pytest.raises(ConfigError):
        engine.parse_number("a corgi")


def test_shout(engine):
    assert engine.shout("a corgi", 1, "upper") == "A CORGI!"
    assert engine.shout("a corgi", 0, "title") == "A Corgi"
    with pytest.raises(ConfigError):
        engine.shout("   ", 1, "upper")


def test_hex_to_text_refuses_what_isnt_utf8(engine):
    with pytest.raises(OpError):
        engine.hex_to_text("ff")


def test_sort_rows(engine):
    table = engine.sample_table(3)
    assert engine.sort_rows(table, "count", True)["columns"][1]["values"] == [7.0, 3.0, 0.0]
    with pytest.raises(ConfigError):
        engine.sort_rows(table, "nothing", False)


def test_ramp_says_what_it_is(engine):
    a = engine.ramp(8, 4)
    assert a.shape == (4, 8, 3) and a.space == "RGB"


def test_normalize(engine):
    assert engine.normalize(np.array([2.0, 4.0, 6.0])).tolist() == [0.0, 0.5, 1.0]
    assert engine.normalize(np.array([3.0, 3.0])).tolist() == [0.0, 0.0], "flat"


def test_shapes_and_recolor(engine):
    drawing = engine.shapes(3, "#112233")
    assert drawing.meta["form"] == {"kind": "vector", "width": 120.0, "height": 40.0}
    assert b'fill="#abcdef"' in engine.recolor(drawing, "#abcdef").data
    with pytest.raises(ConfigError):
        engine.shapes(3, "plum")


def test_checkerboard(engine):
    picture = engine.checkerboard(16, 2)
    assert picture.meta["media"] == "image/png"
    assert engine.image_size(picture) == "16x16"


def test_tone_and_gain(engine):
    tone = engine.tone(440.0, 0.5, 8000)
    assert (tone.frames, tone.channels, tone.sample_rate) == (4000, 1, 8000)
    quieter = engine.gain(tone, -6.0)
    assert quieter.sample_rate == 8000 and abs(quieter.samples).max() < abs(tone.samples).max()


def test_get_key(engine):
    assert engine.get_key(engine.settings("heron", 2), "name") == "heron"
    with pytest.raises(ConfigError):
        engine.get_key({"a": 1}, "b")


def test_split_and_join_list_are_low_level_nodes():
    words = Split().run({"text": "a corgi naps", "separator": " "}, None)["items"]
    assert words.items == ["a", "corgi", "naps"]
    assert JoinList().run({"items": words, "separator": "-"}, None) == {"text": "a-corgi-naps"}
    with pytest.raises(ConfigError):
        Split().run({"text": "x", "separator": ""}, None)
