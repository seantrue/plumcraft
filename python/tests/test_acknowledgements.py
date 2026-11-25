"""Acknowledgements (FR4): an engine's licences, from installed distributions' metadata."""

import pytest

from plumcraft_engine import Engine, ProtoError
from plumcraft_engine.acknowledgements import from_distributions
from plumcraft_engine.host import Host


def test_installed_distributions_give_their_licences_in_full():
    ack = from_distributions(["numpy"])
    names = [p["name"].lower() for g in ack["groups"] for p in g["packages"]]
    assert "numpy" in names
    numpy = next(g for g in ack["groups"] if any(p["name"].lower() == "numpy" for p in g["packages"]))
    # numpy installs the licences of what it bundles too, and all of them are kept (decision 3A)
    assert len(numpy["text"]) > 2000
    assert numpy["licence"] != "unknown"
    assert all(g["text"] for g in ack["groups"])
    assert "note" not in ack


def test_a_distribution_not_installed_is_said():
    ack = from_distributions(["numpy", "no-such-distribution-here"])
    assert ack["note"] == "Not installed: no-such-distribution-here."


class Acked(Engine):
    id = "acked"
    acknowledgements = ("numpy",)


class Plain(Engine):
    id = "plain"


def test_an_engine_answers_acknowledgements_under_its_capability():
    h = Host(Acked)
    try:
        assert h.wait_ready(5)["state"] == "connected"
        hello = h.dispatch("hello", {"v": 2, "op": "hello", "caps": []})
        assert "acknowledgements" in hello["caps"]
        ack = h.dispatch("acknowledgements", {})
        assert any(p["name"].lower() == "numpy" for g in ack["groups"] for p in g["packages"])
    finally:
        h.shutdown()
    h = Host(Plain)
    try:
        assert h.wait_ready(5)["state"] == "connected"
        assert "acknowledgements" not in h.dispatch("hello", {"v": 2, "op": "hello", "caps": []})["caps"]
        with pytest.raises(ProtoError):
            h.dispatch("acknowledgements", {})
    finally:
        h.shutdown()
