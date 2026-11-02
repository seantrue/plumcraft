"""Each node called as a plain method: no host, no PlumCraft, quick to run and to debug."""

import pytest
from plumcraft_engine import ConfigError


def test_add(engine):
    assert engine.add(1.0, 2.5) == 3.5


def test_format_number(engine):
    assert engine.format_number(1.5, 3) == "1.500"


def test_parse_number(engine):
    assert engine.parse_number("2.25") == 2.25
    with pytest.raises(ConfigError):
        engine.parse_number("a corgi")
