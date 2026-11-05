"""Every third-party crate in the given packages' builds, with its licence texts.

    third_party.py ROOT CRATE...   > THIRD-PARTY.txt

Normal dependencies only, for this platform: what a built binary contains. A crate with no
licence file of its own usually has one at its repository's root: `collect_licences.py` keeps those
in `packaging/licences/`, and they are used here. A crate with neither is listed with its licence
expression, repository and authors, and marked, so the gap is seen.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

NAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")
COLLECTED = Path(__file__).parent / "licences"
NESTED = ("LICEN", "COPYING", "NOTICE", "OFL", "UFL")
# Crates that don't name their repository.
REPOSITORIES = {"zune-core": "github.com/etemesi254/zune-image"}


def host() -> str:
    out = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, encoding="utf-8", check=True).stdout
    return next(line.split()[1] for line in out.splitlines() if line.startswith("host:"))


def metadata(root: Path, platform: str) -> dict:
    command = ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", platform]
    return json.loads(subprocess.run(command, cwd=root, capture_output=True, text=True, encoding="utf-8", check=True).stdout)


def repository(package: dict) -> str | None:
    """`host/owner/name` of the crate's repository, without any path inside it."""
    m = re.match(r"https?://(?:www\.)?([^/]+)/([^/]+)/([^/#?]+)", package.get("repository") or "")
    return f"{m[1]}/{m[2]}/{m[3].removesuffix('.git')}" if m else REPOSITORIES.get(package["name"])


def reached(meta: dict, roots: set[str]) -> list[dict]:
    """The packages the roots' normal dependencies reach, without the workspace's own."""
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    todo = [i for i, p in packages.items() if p["name"] in roots and i in meta["workspace_members"]]
    seen: set[str] = set()
    while todo:
        i = todo.pop()
        if i in seen:
            continue
        seen.add(i)
        for dep in nodes[i]["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    found = [packages[i] for i in seen if i not in meta["workspace_members"]]
    return sorted(found, key=lambda p: (p["name"], p["version"]))


def own_licence_files(package: dict) -> list[Path]:
    """The licence files at the crate's root, and in a `LICENSES` folder there."""
    folder = Path(package["manifest_path"]).parent
    files = [p for p in folder.iterdir() if p.is_file() and p.name.upper().startswith(NAMES)]
    if (folder / "LICENSES").is_dir():
        files += [p for p in (folder / "LICENSES").iterdir() if p.is_file()]
    return sorted(files, key=lambda p: p.name)


def nested_licence_files(package: dict) -> list[Path]:
    """Licences deeper in the crate, for what it bundles: fonts, tables, vendored code."""
    folder = Path(package["manifest_path"]).parent
    found = []
    for p in folder.rglob("*"):
        parts = {q.lower() for q in p.relative_to(folder).parts[:-1]}
        if not parts or "licenses" in parts or parts & {"tests", "test", "testdata", "benches", "examples"} or not p.is_file():
            continue
        font_note = "fonts" in parts and p.suffix == ".txt"
        if font_note or any(word in p.name.upper() for word in NESTED):
            found.append(p)
    return sorted(found)


def licence_files(package: dict) -> list[Path]:
    """The crate's licences, or its repository's, and those of what it bundles."""
    own = own_licence_files(package)
    repo = repository(package)
    if not own and repo and (COLLECTED / repo).is_dir():
        own = sorted((p for p in (COLLECTED / repo).iterdir() if p.is_file()), key=lambda p: p.name)
    return own + nested_licence_files(package)


def main() -> int:
    # UTF-8 whatever the console says: Windows writes its code page, which has no "ł"
    sys.stdout.reconfigure(encoding="utf-8")
    root, roots = Path(sys.argv[1]), set(sys.argv[2:])
    packages = reached(metadata(root, host()), roots)
    print(f"Third-party crates in PlumCraft's binaries: {len(packages)}.")
    print("PlumCraft uses them unmodified. The source of each, including those under MPL-2.0, is at")
    print("the repository given, and on crates.io under the name and version below if published there.\n")
    unmarked = 0
    for p in packages:
        print("=" * 78)
        print(f"{p['name']} {p['version']}: {p.get('license') or p.get('license_file') or 'no licence stated'}")
        if p.get("repository"):
            print(p["repository"])
        if p.get("authors"):
            print("Authors: " + ", ".join(p["authors"]))
        files = licence_files(p)
        if not files:
            unmarked += 1
            print("(no licence text is published: the crate declares the licence above)")
        for f in files:
            print(f"\n--- {f.parent.name}/{f.name}\n")
            print(f.read_text("utf-8", "replace").rstrip())
        print()
    print(f"{len(packages)} crates, {unmarked} with no licence file of their own.", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
