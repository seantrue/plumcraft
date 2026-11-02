"""The Python colour spaces (`plumcraft_engine.color`) against impressionist itself, as
`crates/engine/tests/color.rs`: the same fixture (`crates/engine/tests/fixtures/color.json`, from
`claude-debug/color_fixtures_gen.py`), the same per-space tolerances and the same reasons (see the
Rust test), each direction checked on its own; unknown spaces and bad planes are None; RGB ->
space -> RGB recovers in-gamut colours."""

import json
import pathlib

import numpy as np
import pytest

from plumcraft_engine.color import SPACES, from_rgb, to_rgb

FIXTURE = pathlib.Path(__file__).resolve().parents[2] / "crates" / "engine" / "tests" / "fixtures" / "color.json"


def planes(v) -> list[np.ndarray]:
    return [np.array([np.nan if x is None else x for x in p], dtype=np.float32) for p in v]


# (abs per channel, rel, hue wrap, grey threshold), as the Rust test's forward_tol / inverse_tol
FORWARD = {
    "HSI": ((1e-6, 1e-6, 1e-6), 1e-5, 1.0, 0.0),
    "IHSL": ((2e-6, 1e-6, 1e-6), 1e-5, 1.0, 1e-3),
    "HSV": ((1e-4, 1e-6, 1e-6), 1e-5, 360.0, 0.0),
    "HLS": ((1e-4, 1e-6, 1e-6), 1e-5, 360.0, 0.0),
    "Lab": ((0.0062, 0.0157, 0.0157), 0.0, 0.0, 0.0),
    "Luv": ((1e-4, 1e-4, 1e-4), 1e-5, 0.0, 0.0),
}
INVERSE = {"HSI": 2e-6, "IHSL": 2e-6, "Lab": 2e-6, "Luv": 5e-6, "XYZ": 5e-6, "xyY": 5e-6}


def worst(got, want, tol) -> tuple[float, float]:
    """The largest error as a multiple of the tolerance (<= 1 passes), and the largest absolute error."""
    (absolute, rel, wrap, grey) = tol
    ratio_max, abs_max = 0.0, 0.0
    for c, (g, w) in enumerate(zip(got, want, strict=True)):
        g, w = g.astype(np.float64), w.astype(np.float64)
        nan = np.isnan(g) | np.isnan(w)
        assert np.array_equal(np.isnan(g), np.isnan(w)), f"channel {c}: NaN where the reference has none (or the reverse)"
        keep = ~nan
        if c == 0 and grey > 0:
            keep &= want[1].astype(np.float64) >= grey
        e = np.abs(g - w)
        if c == 0 and wrap > 0:
            e = np.minimum(e, wrap - e)
        allowed = absolute[c] + rel * np.abs(w)
        if c == 0 and grey > 0:
            with np.errstate(divide="ignore"):
                allowed = allowed + 1e-7 / want[1].astype(np.float64) ** 2
        if keep.any():
            ratio_max = max(ratio_max, float((e[keep] / allowed[keep]).max()))
            abs_max = max(abs_max, float(e[keep].max()))
    return ratio_max, abs_max


def fixture() -> dict:
    return json.loads(FIXTURE.read_text())


@pytest.mark.parametrize("space", SPACES)
def test_matches_impressionist(space):
    f = fixture()
    rgb = planes(f["rgb"])
    case = f["spaces"][space]
    want_fwd, want_back = planes(case["from_rgb"]), planes(case["to_rgb"])
    fwd = from_rgb(space, rgb)
    back = to_rgb(space, want_fwd)
    rf, _ = worst(fwd, want_fwd, FORWARD.get(space, ((1e-6, 1e-6, 1e-6), 1e-5, 0.0, 0.0)))
    a = INVERSE.get(space, 1e-6)
    rb, _ = worst(back, want_back, ((a, a, a), 1e-5, 0.0, 0.0))
    assert rf <= 1.0, f"{space} from_rgb: {rf:.2f} of the tolerance"
    assert rb <= 1.0, f"{space} to_rgb: {rb:.2f} of the tolerance"


def test_unknown_spaces_and_bad_planes_are_none():
    p = [np.array([0.5], np.float32)] * 3
    for space in ["", "rgb", "CMYK", "lab", "HSL"]:
        assert from_rgb(space, p) is None and to_rgb(space, p) is None
    bad = [[np.zeros(2), np.zeros(1), np.zeros(2)], [np.zeros(1)] * 2, [np.zeros(1)] * 4, []]
    for b in bad:
        for space in SPACES:
            assert from_rgb(space, b) is None and to_rgb(space, b) is None
    empty = from_rgb("Lab", [np.zeros(0, np.float32)] * 3)
    assert [x.size for x in empty] == [0, 0, 0]
    # shapes are kept: (H, W) planes in, (H, W) out
    grid = [np.full((2, 3), v, np.float32) for v in (0.2, 0.4, 0.6)]
    assert all(x.shape == (2, 3) and x.dtype == np.float32 for x in from_rgb("HSV", grid))


def test_round_trip_recovers_in_gamut():
    rgb = planes(fixture()["rgb"])
    keep = np.all([(p >= 0) & (p <= 1) for p in rgb], axis=0) & np.any([p > 0.01 for p in rgb], axis=0)
    rgb = [p[keep] for p in rgb]
    bounds = {"Lab": 2e-3, "YCrCb": 5e-4, "IHSL": 2e-3, "Luv": 1e-4, "HSI": 1e-5, "XYZ": 5e-6, "xyY": 5e-6}
    for space in SPACES:
        back = to_rgb(space, from_rgb(space, rgb))
        err = max(float(np.abs(b - a).max()) for b, a in zip(back, rgb, strict=True))
        assert err <= bounds.get(space, 2e-6), f"{space}: round trip off by {err}"
