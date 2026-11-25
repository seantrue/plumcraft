"""Acknowledgements (FR4, research note 29): the licences of what an engine is built from.

The editor shows them in its Acknowledgements window under the engine's name. An engine gives
them with `Engine.with_acknowledgements` (or the `acknowledgements` class attribute), answering the
`acknowledgements` operation under the capability of that name; one without it shows "no list
given".

`from_distributions` makes the list from installed distributions' metadata: each distribution and
those it requires, with its licence and the full text of every licence file it installed (numpy
and Pillow install the licences of the native libraries they bundle, and all of them are kept).
A list is grouped by licence: each text once, with the distributions under it.
"""

from __future__ import annotations

import importlib.metadata as md
import re
from typing import Any

# The longest text kept from one distribution (characters); the editor bounds a list again.
MAX_TEXT_CHARS = 256 * 1024

_LICENCE_FILE = re.compile(r"^(licen[cs]e|copying|notice|copyright|authors)", re.IGNORECASE)
_REQ_NAME = re.compile(r"^\s*([A-Za-z0-9][A-Za-z0-9._-]*)")


def _norm(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def _licence(dist: md.Distribution) -> str:
    """The distribution's licence: its SPDX expression (PEP 639), else its OSI classifiers, else a
    short `License` field, else "unknown"."""
    meta = dist.metadata
    expr = meta.get("License-Expression")
    if expr:
        return str(expr).strip()
    classifiers = [c.split("::")[-1].strip() for c in meta.get_all("Classifier") or [] if c.startswith("License ::")]
    if classifiers:
        return " OR ".join(dict.fromkeys(classifiers))
    short = (meta.get("License") or "").strip()
    if short and "\n" not in short and len(short) <= 80:
        return short
    return "unknown"


def _url(dist: md.Distribution) -> str:
    meta = dist.metadata
    urls = {}
    for line in meta.get_all("Project-URL") or []:
        label, _, url = line.partition(",")
        urls[label.strip().lower()] = url.strip()
    for key in ("source", "source code", "repository", "code", "homepage", "home"):
        if urls.get(key):
            return urls[key]
    return (meta.get("Home-page") or "").strip()


def _texts(dist: md.Distribution) -> str:
    """Every licence file the distribution installed, in full, each under its name: those in its
    `.dist-info/licenses/` (PEP 639) and those named like a licence in its `.dist-info`."""
    found = []
    for f in dist.files or []:
        parts = f.parts
        in_info = len(parts) >= 2 and parts[0].endswith(".dist-info")
        if not in_info:
            continue
        if "licenses" in parts[1:-1] or (len(parts) == 2 and _LICENCE_FILE.match(parts[-1])):
            try:
                text = f.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError):
                continue
            if text and text.strip():
                found.append(("/".join(parts[1:]), text.replace("\r\n", "\n")))
    found.sort()
    if len(found) == 1:
        text = found[0][1]
    else:
        text = "\n\n".join(f"── {name} ──\n\n{t.strip()}" for name, t in found)
    if len(text) > MAX_TEXT_CHARS:
        text = text[:MAX_TEXT_CHARS] + "\n\n[… cut: the text is longer than the list keeps]"
    return text


def _required(dist: md.Distribution) -> list[str]:
    """The distributions `dist` needs, leaving out those only an extra asks for."""
    out = []
    for req in dist.requires or []:
        _, _, marker = req.partition(";")
        if re.search(r"\bextra\b", marker):
            continue
        m = _REQ_NAME.match(req)
        if m:
            out.append(m.group(1))
    return out


def from_distributions(names: list[str] | tuple[str, ...], *, requirements: bool = True) -> dict[str, Any]:
    """The licences of the installed distributions `names` (and, with `requirements`, of every
    installed distribution they require, in turn), grouped by licence and text. A name not
    installed is left out and said in the list's `note`."""
    seen: dict[str, md.Distribution] = {}
    missing: list[str] = []
    todo = list(names)
    while todo:
        name = todo.pop(0)
        key = _norm(name)
        if key in seen:
            continue
        try:
            dist = md.distribution(name)
        except md.PackageNotFoundError:
            if name in names:
                missing.append(name)
            continue
        seen[key] = dist
        if requirements:
            todo.extend(_required(dist))
    groups: dict[tuple[str, str], dict[str, Any]] = {}
    for dist in seen.values():
        meta = dist.metadata
        licence = _licence(dist)
        text = _texts(dist) or f"{meta.get('Name', '')} installed no licence file; its metadata says: {licence}."
        g = groups.setdefault((licence, text), {"licence": licence, "text": text, "packages": []})
        g["packages"].append({"name": meta.get("Name", ""), "version": meta.get("Version", ""), "url": _url(dist)})
    out = sorted(groups.values(), key=lambda g: (g["licence"].lower(), g["packages"][0]["name"].lower()))
    for g in out:
        g["packages"].sort(key=lambda p: p["name"].lower())
    note = f"Not installed: {', '.join(missing)}." if missing else ""
    return {"groups": out, "note": note} if note else {"groups": out}
