"""Every conversion ships with its inverse: going there and back gives what you started with."""

import pytest

from weather_engine import JoinList, Split


@pytest.mark.parametrize("x", [0.0, 1.5, -2.25, 1000.0])
def test_format_and_parse_number(engine, x):
    assert engine.parse_number(engine.format_number(x, 6)) == x


@pytest.mark.parametrize("text", ["", "a corgi", "ünï çødé 🐕"])
def test_text_to_hex_and_back(engine, text):
    assert engine.hex_to_text(engine.text_to_hex(text)) == text


@pytest.mark.parametrize("text", ["a corgi naps", "one"])
def test_split_and_join(text):
    assert JoinList().run({"items": Split().run({"text": text, "separator": " "}, None)["items"], "separator": " "}, None) == {"text": text}


def test_table_to_csv_and_back(engine):
    table = engine.sample_table(4)
    assert engine.csv_to_table(engine.table_to_csv(table)) == table


def test_table_to_array_and_back(engine):
    table = {"columns": [{"name": "a", "type": "FLOAT", "values": [1.0, 2.5]}, {"name": "b", "type": "FLOAT", "values": [3.0, 4.0]}]}
    assert engine.array_to_table(engine.table_to_array(table)) == table


def test_svg_to_text_and_back(engine):
    drawing = engine.shapes(2, "#112233")
    assert engine.text_to_svg(engine.svg_to_text(drawing)).data == drawing.data


def test_image_to_array_and_back(engine):
    picture = engine.array_to_image(engine.ramp(8, 8))
    assert engine.array_to_image(engine.image_to_array(picture)).data == picture.data, "the same pixels, 8 bits a channel"


def test_audio_to_array_and_back(engine):
    tone = engine.tone(440.0, 0.1, 8000)
    back = engine.array_to_audio(engine.audio_to_array(tone), 8000)
    assert back.sample_rate == 8000 and (back.samples == tone.samples).all()


def test_json_to_text_and_back(engine):
    value = engine.settings("heron", 2)
    assert engine.text_to_json(engine.json_to_text(value)) == value
