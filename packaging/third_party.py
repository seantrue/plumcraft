"""Every third-party crate in the given packages' builds, with its licence texts.

    third_party.py ROOT CRATE...   > THIRD-PARTY.txt

Normal dependencies only, for this platform: what a built binary contains. A crate with no
licence file of its own, usually because its repository keeps one at the root, is listed with its
licence expression, repository and authors, and marked, so the gap is seen.
"""

import json
import subprocess
import sys
from pathlib import Path

NAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")


def host() -> str:
    out = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout
    return next(line.split()[1] for line in out.splitlines() if line.startswith("host:"))


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


def licence_files(package: dict) -> list[Path]:
    folder = Path(package["manifest_path"]).parent
    files = [p for p in folder.iterdir() if p.is_file() and p.name.upper().startswith(NAMES)]
    return sorted(files, key=lambda p: p.name)


def main() -> int:
    root, roots = Path(sys.argv[1]), set(sys.argv[2:])
    command = ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", host()]
    meta = json.loads(subprocess.run(command, cwd=root, capture_output=True, text=True, check=True).stdout)
    packages = reached(meta, roots)
    print(f"Third-party crates in PlumCraft's binaries: {len(packages)}.\n")
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
            print("(the crate ships no licence file)")
        for f in files:
            print(f"\n--- {f.name}\n")
            print(f.read_text("utf-8", "replace").rstrip())
        print()
    print(f"{len(packages)} crates, {unmarked} with no licence file of their own.", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
