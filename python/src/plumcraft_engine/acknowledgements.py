"""Acknowledgements (FR4, research note 29): the licences of what an engine is built from.

The editor shows them in its Acknowledgements window under the engine's name. An engine gives
them with `Engine.with_acknowledgements` (or the `acknowledgements` class attribute), answering the
`acknowledgements` operation under the capability of that name; one without it shows "no list
given".

`from_distributions` makes the list from installed distributions' metadata: each distribution and
those it requires, with its licence and the full text of every licence file it installed (numpy
and Pillow install the licences of the native libraries they bundle, and all of them are kept).
A list is grouped by licence: each text once, with the distributions under it.

The licence is an SPDX id or expression (spec 6.3), so the editor's one row per licence id holds
every package under it (B182: "MIT License" from a classifier beside "MIT" made two). Without a
`License-Expression`, the classifiers and the free-form `License` field are mapped through the
tables below, offline; one that names no single licence ("BSD License") is not guessed, and the
distribution is under `LicenseRef-unknown` with the metadata's own words in its text.
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


# What a licence is called when the metadata names none that maps to one SPDX id (spec 6.3: an
# SPDX id or expression, `LicenseRef-…` for terms with none). The metadata's own words go in the
# text, so nothing is guessed and nothing is lost.
UNKNOWN = "LicenseRef-unknown"
UNKNOWN_NAME = "Licence not stated as an SPDX id"

# The SPDX ids this list maps to, with their names (SPDX License List): the canonical spelling of
# an id given in any case, and the group's `name`.
SPDX_NAMES = {
    "0BSD": "BSD Zero Clause License",
    "AAL": "Attribution Assurance License",
    "AFL-3.0": "Academic Free License v3.0",
    "AGPL-3.0-only": "GNU Affero General Public License v3.0 only",
    "AGPL-3.0-or-later": "GNU Affero General Public License v3.0 or later",
    "Aladdin": "Aladdin Free Public License",
    "Apache-1.1": "Apache License 1.1",
    "Apache-2.0": "Apache License 2.0",
    "Artistic-2.0": "Artistic License 2.0",
    "BlueOak-1.0.0": "Blue Oak Model License 1.0.0",
    "BSD-1-Clause": "BSD 1-Clause License",
    "BSD-2-Clause": 'BSD 2-Clause "Simplified" License',
    "BSD-3-Clause": 'BSD 3-Clause "New" or "Revised" License',
    "BSD-3-Clause-Clear": "BSD 3-Clause Clear License",
    "BSD-4-Clause": 'BSD 4-Clause "Original" or "Old" License',
    "BSL-1.0": "Boost Software License 1.0",
    "CC-BY-4.0": "Creative Commons Attribution 4.0 International",
    "CC-BY-SA-4.0": "Creative Commons Attribution Share Alike 4.0 International",
    "CC0-1.0": "Creative Commons Zero v1.0 Universal",
    "CDDL-1.0": "Common Development and Distribution License 1.0",
    "CECILL-2.1": "CeCILL Free Software License Agreement v2.1",
    "CECILL-B": "CeCILL-B Free Software License Agreement",
    "CECILL-C": "CeCILL-C Free Software License Agreement",
    "CNRI-Python": "CNRI Python License",
    "CPL-1.0": "Common Public License 1.0",
    "curl": "curl License",
    "ECL-2.0": "Educational Community License v2.0",
    "EPL-1.0": "Eclipse Public License 1.0",
    "EPL-2.0": "Eclipse Public License 2.0",
    "EUPL-1.0": "European Union Public License 1.0",
    "EUPL-1.1": "European Union Public License 1.1",
    "EUPL-1.2": "European Union Public License 1.2",
    "GPL-2.0-only": "GNU General Public License v2.0 only",
    "GPL-2.0-or-later": "GNU General Public License v2.0 or later",
    "GPL-3.0-only": "GNU General Public License v3.0 only",
    "GPL-3.0-or-later": "GNU General Public License v3.0 or later",
    "HPND": "Historical Permission Notice and Disclaimer",
    "Intel": "Intel Open Source License",
    "IPL-1.0": "IBM Public License v1.0",
    "ISC": "ISC License",
    "LGPL-2.0-only": "GNU Library General Public License v2 only",
    "LGPL-2.0-or-later": "GNU Library General Public License v2 or later",
    "LGPL-2.1-only": "GNU Lesser General Public License v2.1 only",
    "LGPL-2.1-or-later": "GNU Lesser General Public License v2.1 or later",
    "LGPL-3.0-only": "GNU Lesser General Public License v3.0 only",
    "LGPL-3.0-or-later": "GNU Lesser General Public License v3.0 or later",
    "MirOS": "The MirOS Licence",
    "MIT": "MIT License",
    "MIT-0": "MIT No Attribution",
    "MIT-CMU": "CMU License",
    "Motosoto": "Motosoto License",
    "MPL-1.0": "Mozilla Public License 1.0",
    "MPL-1.1": "Mozilla Public License 1.1",
    "MPL-2.0": "Mozilla Public License 2.0",
    "MulanPSL-2.0": "Mulan Permissive Software License, Version 2",
    "NCSA": "University of Illinois/NCSA Open Source License",
    "NGPL": "Nethack General Public License",
    "Nokia": "Nokia Open Source License",
    "OFL-1.1": "SIL Open Font License 1.1",
    "OGTSL": "Open Group Test Suite License",
    "OpenSSL": "OpenSSL License",
    "OSL-3.0": "Open Software License 3.0",
    "PostgreSQL": "PostgreSQL License",
    "PSF-2.0": "Python Software Foundation License 2.0",
    "Python-2.0": "Python License 2.0",
    "QPL-1.0": "Q Public License 1.0",
    "RSCPL": "Ricoh Source Code Public License",
    "SISSL": "Sun Industry Standards Source License v1.1",
    "Sleepycat": "Sleepycat License",
    "SPL-1.0": "Sun Public License v1.0",
    "Unicode-3.0": "Unicode License v3",
    "Unicode-DFS-2016": "Unicode License Agreement - Data Files and Software (2016)",
    "Unlicense": "The Unlicense",
    "UPL-1.0": "Universal Permissive License v1.0",
    "VSL-1.0": "Vovida Software License v1.0",
    "W3C": "W3C Software Notice and License (2002-12-31)",
    "WTFPL": "Do What The F*ck You Want To Public License",
    "X11": "X11 License",
    "Xnet": "X.Net License",
    "Zlib": "zlib License",
    "ZPL-2.1": "Zope Public License 2.1",
}
_CANON = {k.lower(): k for k in SPDX_NAMES}
# SPDX ids deprecated in favour of `-only` and `-or-later`, which a free-form field often holds
_CANON |= {
    f"{g}-{v}{plus}": f"{g}-{v}-{'or-later' if plus else 'only'}"
    for g, vs in (("gpl", ("2.0", "3.0")), ("lgpl", ("2.0", "2.1", "3.0")), ("agpl", ("3.0",)))
    for v in vs
    for plus in ("", "+")
}

# The trove classifiers (after `License ::`) and the one SPDX id each names. A classifier that
# names no single licence ("BSD License", "GNU General Public License (GPL)", "Artistic License")
# is not here: it decides nothing, and is not guessed. `Apache Software License` names no
# version, but Apache-2.0 is the only one in use since 2004 and every reader takes it so.
CLASSIFIERS = {
    "Aladdin Free Public License (AFPL)": "Aladdin",
    "CC0 1.0 Universal (CC0 1.0) Public Domain Dedication": "CC0-1.0",
    "CeCILL-B Free Software License Agreement (CECILL-B)": "CECILL-B",
    "CeCILL-C Free Software License Agreement (CECILL-C)": "CECILL-C",
    "Nokia Open Source License (NOKOS)": "Nokia",
    "OSI Approved :: Apache Software License": "Apache-2.0",
    "OSI Approved :: Attribution Assurance License": "AAL",
    "OSI Approved :: Boost Software License 1.0 (BSL-1.0)": "BSL-1.0",
    "OSI Approved :: CEA CNRS Inria Logiciel Libre License, version 2.1 (CeCILL-2.1)": "CECILL-2.1",
    "OSI Approved :: Common Public License": "CPL-1.0",
    "OSI Approved :: Eclipse Public License 1.0 (EPL-1.0)": "EPL-1.0",
    "OSI Approved :: Eclipse Public License 2.0 (EPL-2.0)": "EPL-2.0",
    "OSI Approved :: European Union Public Licence 1.0 (EUPL 1.0)": "EUPL-1.0",
    "OSI Approved :: European Union Public Licence 1.1 (EUPL 1.1)": "EUPL-1.1",
    "OSI Approved :: European Union Public Licence 1.2 (EUPL 1.2)": "EUPL-1.2",
    "OSI Approved :: GNU Affero General Public License v3": "AGPL-3.0-only",
    "OSI Approved :: GNU Affero General Public License v3 or later (AGPLv3+)": "AGPL-3.0-or-later",
    "OSI Approved :: GNU General Public License v2 (GPLv2)": "GPL-2.0-only",
    "OSI Approved :: GNU General Public License v2 or later (GPLv2+)": "GPL-2.0-or-later",
    "OSI Approved :: GNU General Public License v3 (GPLv3)": "GPL-3.0-only",
    "OSI Approved :: GNU General Public License v3 or later (GPLv3+)": "GPL-3.0-or-later",
    "OSI Approved :: GNU Lesser General Public License v2 (LGPLv2)": "LGPL-2.0-only",
    "OSI Approved :: GNU Lesser General Public License v2 or later (LGPLv2+)": "LGPL-2.0-or-later",
    "OSI Approved :: GNU Lesser General Public License v3 (LGPLv3)": "LGPL-3.0-only",
    "OSI Approved :: GNU Lesser General Public License v3 or later (LGPLv3+)": "LGPL-3.0-or-later",
    "OSI Approved :: Historical Permission Notice and Disclaimer (HPND)": "HPND",
    "OSI Approved :: IBM Public License": "IPL-1.0",
    "OSI Approved :: ISC License (ISCL)": "ISC",
    "OSI Approved :: Intel Open Source License": "Intel",
    "OSI Approved :: MIT License": "MIT",
    "OSI Approved :: MIT No Attribution License (MIT-0)": "MIT-0",
    "OSI Approved :: MirOS License (MirOS)": "MirOS",
    "OSI Approved :: Motosoto License": "Motosoto",
    "OSI Approved :: Mozilla Public License 1.0 (MPL)": "MPL-1.0",
    "OSI Approved :: Mozilla Public License 1.1 (MPL 1.1)": "MPL-1.1",
    "OSI Approved :: Mozilla Public License 2.0 (MPL 2.0)": "MPL-2.0",
    "OSI Approved :: Nethack General Public License": "NGPL",
    "OSI Approved :: Nokia Open Source License": "Nokia",
    "OSI Approved :: Open Group Test Suite License": "OGTSL",
    "OSI Approved :: PostgreSQL License": "PostgreSQL",
    "OSI Approved :: Python License (CNRI Python License)": "CNRI-Python",
    "OSI Approved :: Python Software Foundation License": "PSF-2.0",
    "OSI Approved :: Qt Public License (QPL)": "QPL-1.0",
    "OSI Approved :: Ricoh Source Code Public License": "RSCPL",
    "OSI Approved :: SIL Open Font License 1.1 (OFL-1.1)": "OFL-1.1",
    "OSI Approved :: Sleepycat License": "Sleepycat",
    "OSI Approved :: Sun Industry Standards Source License (SISSL)": "SISSL",
    "OSI Approved :: Sun Public License": "SPL-1.0",
    "OSI Approved :: The Unlicense (Unlicense)": "Unlicense",
    "OSI Approved :: Universal Permissive License (UPL)": "UPL-1.0",
    "OSI Approved :: University of Illinois/NCSA Open Source License": "NCSA",
    "OSI Approved :: Vovida Software License 1.0": "VSL-1.0",
    "OSI Approved :: W3C License": "W3C",
    "OSI Approved :: X.Net License": "Xnet",
    "OSI Approved :: Zero-Clause BSD (0BSD)": "0BSD",
    "OSI Approved :: zlib/libpng License": "Zlib",
}
_CLASSIFIERS = {k.lower(): v for k, v in CLASSIFIERS.items()}
# Classifiers that say something about a licence but not which: passed over beside one that does
_SAYS_NOTHING = {"osi approved", "dfsg approved", "public domain"}

# Common free-form `License` fields (lower case, spaces collapsed) and the one SPDX id each names.
# As with the classifiers, a bare "BSD" or "GPL" is not here.
FIELDS = {
    "mit license": "MIT",
    "the mit license": "MIT",
    "mit-0": "MIT-0",
    "apache": "Apache-2.0",
    "apache 2": "Apache-2.0",
    "apache 2.0": "Apache-2.0",
    "apache-2": "Apache-2.0",
    "apache license": "Apache-2.0",
    "apache license 2.0": "Apache-2.0",
    "apache license, version 2.0": "Apache-2.0",
    "apache license version 2.0": "Apache-2.0",
    "apache software license": "Apache-2.0",
    "apache software license 2.0": "Apache-2.0",
    "asl 2.0": "Apache-2.0",
    "bsd 2-clause": "BSD-2-Clause",
    "bsd-2": "BSD-2-Clause",
    "2-clause bsd": "BSD-2-Clause",
    "simplified bsd": "BSD-2-Clause",
    "freebsd": "BSD-2-Clause",
    "bsd 3-clause": "BSD-3-Clause",
    "bsd-3": "BSD-3-Clause",
    "3-clause bsd": "BSD-3-Clause",
    "new bsd": "BSD-3-Clause",
    "new bsd license": "BSD-3-Clause",
    "modified bsd": "BSD-3-Clause",
    "revised bsd": "BSD-3-Clause",
    "isc license": "ISC",
    "mpl 2.0": "MPL-2.0",
    "mpl-2": "MPL-2.0",
    "mozilla public license 2.0": "MPL-2.0",
    "gplv2": "GPL-2.0-only",
    "gplv2+": "GPL-2.0-or-later",
    "gplv3": "GPL-3.0-only",
    "gplv3+": "GPL-3.0-or-later",
    "lgplv2.1": "LGPL-2.1-only",
    "lgplv2.1+": "LGPL-2.1-or-later",
    "lgplv3": "LGPL-3.0-only",
    "lgplv3+": "LGPL-3.0-or-later",
    "agplv3": "AGPL-3.0-only",
    "agplv3+": "AGPL-3.0-or-later",
    "psf": "PSF-2.0",
    "psf license": "PSF-2.0",
    "psf-2": "PSF-2.0",
    "python software foundation license": "PSF-2.0",
    "the unlicense": "Unlicense",
    "zlib license": "Zlib",
    "zlib/libpng": "Zlib",
    "boost software license 1.0": "BSL-1.0",
    "cc0": "CC0-1.0",
    "cc0 1.0": "CC0-1.0",
}
_EXPR_TOKEN = re.compile(r"\(|\)|[^\s()]+")
_REF = re.compile(r"^(DocumentRef-[A-Za-z0-9.-]+:)?LicenseRef-[A-Za-z0-9.-]+$")
_EXCEPTION = re.compile(r"^[A-Za-z0-9.+-]+$")


def _expression(text: str) -> str | None:
    """`text` as an SPDX expression of known ids, each spelt as SPDX spells it, else None."""
    out: list[str] = []
    after_with = False
    for tok in _EXPR_TOKEN.findall(text):
        up = tok.upper()
        if tok in "()":
            out.append(tok)
        elif up in ("AND", "OR", "WITH"):
            if not out or out[-1] in ("AND", "OR", "WITH", "("):
                return None
            out.append(up)
            after_with = up == "WITH"
            continue
        elif after_with:
            if not _EXCEPTION.match(tok):
                return None
            out.append(tok)
        elif _REF.match(tok):
            out.append(tok)
        elif tok.lower() in _CANON:
            out.append(_CANON[tok.lower()])
        else:
            return None
        after_with = False
    if not out or out[-1] in ("AND", "OR", "WITH"):
        return None
    return " ".join(out).replace("( ", "(").replace(" )", ")")


def _field(text: str) -> str | None:
    """A free-form `License` field as an SPDX id or expression, when it names one."""
    short = " ".join(text.split())
    if not short or "\n" in text.strip() or len(short) > 80:
        return None
    return _expression(short) or FIELDS.get(short.lower())


def _classifiers(dist: md.Distribution) -> list[str]:
    return [c.split("::", 1)[1].strip() for c in dist.metadata.get_all("Classifier") or [] if c.startswith("License ::")]


def _licence(dist: md.Distribution) -> str:
    """The distribution's licence as an SPDX id or expression (spec 6.3): its `License-Expression`
    (PEP 639); else its licence classifiers, when every one names a single id (several are joined
    with OR, as a dual licence); else its `License` field, when that is an SPDX expression or a
    common name for one; else `LicenseRef-unknown`."""
    meta = dist.metadata
    expr = meta.get("License-Expression")
    if expr and str(expr).strip():
        return " ".join(str(expr).split())
    named = [c for c in _classifiers(dist) if c.lower() not in _SAYS_NOTHING]
    ids = [_CLASSIFIERS.get(c.lower()) or _in_parens(c) for c in named]
    if ids and all(ids):
        return " OR ".join(dict.fromkeys(i for i in ids if i))
    return _field(meta.get("License") or "") or UNKNOWN


def _in_parens(classifier: str) -> str | None:
    """A classifier that ends with an SPDX id in parentheses names it ("… (BSD-3-Clause)")."""
    m = re.search(r"\(([^()]+)\)$", classifier)
    return _CANON.get(m.group(1).strip().lower()) if m else None


def _stated(dist: md.Distribution) -> str:
    """What the metadata says of the licence, in its own words, for a licence that maps to no id."""
    meta = dist.metadata
    said = [f"“{c}”" for c in _classifiers(dist)]
    field = " ".join((meta.get("License") or "").split())
    if field:
        said.append(f"“{field[:200]}{'…' if len(field) > 200 else ''}”")
    return ", ".join(said) if said else "nothing"


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
        said = _stated(dist) if licence == UNKNOWN else licence
        text = _texts(dist) or f"{meta.get('Name', '')} installed no licence file; its metadata says: {said}."
        name = UNKNOWN_NAME if licence == UNKNOWN else SPDX_NAMES.get(licence, "")
        g = groups.setdefault((licence, text), {"licence": licence, **({"name": name} if name else {}), "text": text, "packages": []})
        g["packages"].append({"name": meta.get("Name", ""), "version": meta.get("Version", ""), "url": _url(dist)})
    out = sorted(groups.values(), key=lambda g: (g["licence"].lower(), g["packages"][0]["name"].lower()))
    for g in out:
        g["packages"].sort(key=lambda p: p["name"].lower())
    note = f"Not installed: {', '.join(missing)}." if missing else ""
    return {"groups": out, "note": note} if note else {"groups": out}
