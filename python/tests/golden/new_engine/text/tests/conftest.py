"""Shared by the tests: this engine, one instance, and its demo workflow."""

import json
from pathlib import Path

import pytest
from plumcraft_engine.cli import anchored

from weather_engine import WeatherEngine

ROOT = Path(__file__).resolve().parent.parent


@pytest.fixture(scope="session")
def engine() -> WeatherEngine:
    return WeatherEngine()


@pytest.fixture(scope="session")
def demo(engine) -> dict:
    """The demo, its relative paths made absolute against its folder, as `plumcraft-engine run` and the editor do."""
    folder = ROOT / "examples"
    return anchored(json.loads((folder / "demo.json").read_text("utf-8")), engine.schema(), folder)
