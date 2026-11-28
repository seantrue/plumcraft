"""Each node called as a plain method: no host, no PlumCraft, quick to run and to debug."""

import pytest
from plumcraft_engine import ConfigError, OpError


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
