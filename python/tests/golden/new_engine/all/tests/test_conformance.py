"""PlumCraft's conformance checks: what every engine must do, in process and over HTTP."""

from plumcraft_engine.conformance import assert_conformant

from weather_engine import WeatherEngine


def test_the_engine_is_conformant(demo):
    assert_conformant(WeatherEngine, demo=demo, slow="weather.slow")
