"""Every conversion ships with its inverse: going there and back gives what you started with."""

import pytest


@pytest.mark.parametrize("x", [0.0, 1.5, -2.25, 1000.0])
def test_format_and_parse_number(engine, x):
    assert engine.parse_number(engine.format_number(x, 6)) == x


@pytest.mark.parametrize("text", ["", "a corgi", "ünï çødé 🐕"])
def test_text_to_hex_and_back(engine, text):
    assert engine.hex_to_text(engine.text_to_hex(text)) == text
