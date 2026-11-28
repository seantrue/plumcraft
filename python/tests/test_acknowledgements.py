"""Acknowledgements (FR4): an engine's licences, from installed distributions' metadata."""

import importlib.metadata as md

import pytest

import plumcraft_engine.acknowledgements as ack_module
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


# ---------- B182: the licence is an SPDX id or expression (spec 6.3), from fake distributions ----------


class FakeDist(md.Distribution):
    """A distribution with only the metadata given: no files, no requirements."""

    def __init__(self, name: str, *, expression: str = "", classifiers: tuple[str, ...] = (), field: str = ""):
        lines = ["Metadata-Version: 2.4", f"Name: {name}", "Version: 1.0"]
        lines += [f"License-Expression: {expression}"] if expression else []
        lines += [f"License: {field}"] if field else []
        lines += [f"Classifier: {c}" for c in classifiers]
        self._meta = "\n".join(lines) + "\n\n"

    def read_text(self, filename):
        return self._meta if filename in ("METADATA", "PKG-INFO") else None

    def locate_file(self, path):
        return path


def acked(monkeypatch, *dists: FakeDist) -> dict:
    by_name = {d.metadata["Name"]: d for d in dists}

    def distribution(name):
        if name not in by_name:
            raise md.PackageNotFoundError(name)
        return by_name[name]

    monkeypatch.setattr(ack_module.md, "distribution", distribution)
    return from_distributions(list(by_name))


def licences(ack: dict) -> dict[str, str]:
    return {p["name"]: g["licence"] for g in ack["groups"] for p in g["packages"]}


OSI = "License :: OSI Approved :: "


@pytest.mark.parametrize(
    "kw, want",
    [
        # PEP 639's expression is taken as it is
        ({"expression": "Apache-2.0 OR BSD-3-Clause"}, "Apache-2.0 OR BSD-3-Clause"),
        ({"expression": "MIT", "classifiers": (OSI + "Apache Software License",)}, "MIT"),
        # the trove classifiers, mapped
        ({"classifiers": (OSI + "MIT License",)}, "MIT"),
        ({"classifiers": (OSI + "Apache Software License",)}, "Apache-2.0"),
        ({"classifiers": (OSI + 'BSD 3-Clause "New" or "Revised" License (BSD-3-Clause)',)}, "BSD-3-Clause"),
        ({"classifiers": (OSI + "ISC License (ISCL)",)}, "ISC"),
        ({"classifiers": (OSI + "Mozilla Public License 2.0 (MPL 2.0)",)}, "MPL-2.0"),
        ({"classifiers": (OSI + "GNU Lesser General Public License v3 (LGPLv3)",)}, "LGPL-3.0-only"),
        ({"classifiers": (OSI + "GNU General Public License v2 or later (GPLv2+)",)}, "GPL-2.0-or-later"),
        ({"classifiers": (OSI + "Python Software Foundation License",)}, "PSF-2.0"),
        ({"classifiers": ("License :: Public Domain", OSI + "The Unlicense (Unlicense)")}, "Unlicense"),
        ({"classifiers": (OSI + "zlib/libpng License",)}, "Zlib"),
        ({"classifiers": (OSI + "MIT License", OSI + "Apache Software License")}, "MIT OR Apache-2.0"),
        # a bare "OSI Approved" says nothing more, and is passed over
        ({"classifiers": ("License :: OSI Approved", OSI + "MIT License")}, "MIT"),
        # the free-form field: an SPDX id in any case, or a common name
        ({"field": "mit"}, "MIT"),
        ({"field": "Apache 2.0"}, "Apache-2.0"),
        ({"field": "Apache License, Version 2.0"}, "Apache-2.0"),
        ({"field": "BSD 3-Clause"}, "BSD-3-Clause"),
        ({"field": "MIT OR Apache-2.0"}, "MIT OR Apache-2.0"),
        # a classifier that names no one licence is not guessed: the field decides, else unknown
        ({"classifiers": (OSI + "BSD License",), "field": "BSD-3-Clause"}, "BSD-3-Clause"),
        ({"classifiers": (OSI + "BSD License",)}, "LicenseRef-unknown"),
        ({"classifiers": (OSI + "BSD License",), "field": "BSD"}, "LicenseRef-unknown"),
        ({"classifiers": ("License :: Other/Proprietary License",)}, "LicenseRef-unknown"),
        ({"field": "Copyright the authors, all rights reserved"}, "LicenseRef-unknown"),
        ({}, "LicenseRef-unknown"),
    ],
)
def test_the_licence_is_an_spdx_id_or_expression(monkeypatch, kw, want):
    assert licences(acked(monkeypatch, FakeDist("pkg", **kw))) == {"pkg": want}


def test_a_classifier_and_an_spdx_id_fall_in_one_group(monkeypatch):
    # the editor makes one row per licence id: "MIT License" beside "MIT" would be two rows
    ack = acked(
        monkeypatch,
        FakeDist("by-classifier", classifiers=(OSI + "MIT License",)),
        FakeDist("by-expression", expression="MIT"),
        FakeDist("by-field", field="MIT License"),
    )
    assert {g["licence"] for g in ack["groups"]} == {"MIT"}
    assert all(g.get("name") == "MIT License" for g in ack["groups"])


def test_an_unknown_licence_says_what_the_metadata_said(monkeypatch):
    ack = acked(monkeypatch, FakeDist("vague", classifiers=(OSI + "BSD License",)))
    (g,) = ack["groups"]
    assert g["licence"] == "LicenseRef-unknown"
    assert "BSD License" in g["text"]
