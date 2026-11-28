"""The engine as PlumCraft sees it. The golden catalogue catches a renamed port, which would break
saved workflows: when a change is meant, `UPDATE_GOLDEN=1 uv run pytest` rewrites it."""

import json
import os
from pathlib import Path

from plumcraft_engine.cli import Local
from plumcraft_engine.conformance import Run

from weather_engine import WeatherEngine

GOLDEN = Path(__file__).parent / "golden" / "schema.json"


def test_the_catalogue_matches_the_golden(engine):
    schema = engine.schema()
    got = {"wire_types": schema["wire_types"], "nodes": schema["nodes"]}
    if os.environ.get("UPDATE_GOLDEN"):
        GOLDEN.write_text(json.dumps(got, indent=2, sort_keys=True) + "\n")
    assert got == json.loads(GOLDEN.read_text("utf-8"))


def test_the_demo_runs_every_node(demo):
    conn = Local(WeatherEngine)
    try:
        states = Run(conn, demo, "demo").states()
    finally:
        conn.close()
    assert states == {n: "done" for n in demo["nodes"]}
