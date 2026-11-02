"""Fetch the root licence files of repositories whose crates ship none, into `packaging/licences/`.

    collect_licences.py

Run it when dependencies change; `third_party.py` then uses what is here, so a release build stays
offline and repeatable. GitHub through `gh`, Codeberg through its API. Prints what it couldn't find.
"""

import base64
import json
import subprocess
import sys
import urllib.request
from pathlib import Path

from third_party import COLLECTED, NAMES, metadata, own_licence_files, reached, repository

ROOT = Path(__file__).resolve().parent.parent
PLATFORMS = ("aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu")
CRATES = {"plumcraft-editor", "plumcraft", "plumcraft-text", "plumcraft-array", "plumcraft-audio", "plumcraft-craft"}
# Checked by hand: these repositories hold no licence file. Their Cargo.toml declares the licence,
# and that is all there is to carry.
NONE_PUBLISHED = {"dispatch", "egui-scale"}


def api(url: str) -> object:
    with urllib.request.urlopen(url, timeout=30) as reply:
        return json.load(reply)


def github(owner: str, name: str) -> dict[str, bytes]:
    run = lambda path: json.loads(subprocess.run(["gh", "api", path], capture_output=True, text=True, check=True).stdout)  # noqa: E731
    found = {}
    for entry in run(f"repos/{owner}/{name}/contents"):
        if entry["type"] == "file" and entry["name"].upper().startswith(NAMES):
            found[entry["name"]] = base64.b64decode(run(f"repos/{owner}/{name}/contents/{entry['name']}")["content"])
    return found


def codeberg(owner: str, name: str) -> dict[str, bytes]:
    base = f"https://codeberg.org/api/v1/repos/{owner}/{name}/contents"
    found = {}
    for entry in api(base):
        if entry["type"] == "file" and entry["name"].upper().startswith(NAMES):
            found[entry["name"]] = base64.b64decode(api(f"{base}/{entry['name']}")["content"])
    return found


def fetch(repo: str) -> dict[str, bytes]:
    host, owner, name = repo.split("/")
    if host == "github.com":
        return github(owner, name)
    if host == "codeberg.org":
        return codeberg(owner, name)
    return {}


def main() -> int:
    wanted: dict[str, set[str]] = {}
    unknown: set[str] = set()
    for platform in PLATFORMS:
        for package in reached(metadata(ROOT, platform), CRATES):
            if own_licence_files(package):
                continue
            repo = repository(package)
            if repo:
                wanted.setdefault(repo, set()).add(package["name"])
            else:
                unknown.add(package["name"])
    missing = sorted(unknown)
    for repo, crates in sorted(wanted.items()):
        try:
            files = fetch(repo)
        except (subprocess.CalledProcessError, OSError, ValueError) as e:
            print(f"{repo}: {e}", file=sys.stderr)
            files = {}
        if not files:
            missing.extend(sorted(crates))
            print(f"none at {repo}: {', '.join(sorted(crates))}")
            continue
        folder = COLLECTED / repo
        folder.mkdir(parents=True, exist_ok=True)
        for name, data in files.items():
            (folder / name).write_bytes(data)
        print(f"{repo}: {', '.join(sorted(files))} ({len(crates)} crates)")
    unexplained = sorted(set(missing) - NONE_PUBLISHED)
    print(f"\n{len(wanted)} repositories; crates declaring a licence with no text published: {sorted(NONE_PUBLISHED)}")
    print(f"crates still unexplained: {unexplained or 'none'}")
    return 1 if unexplained else 0


if __name__ == "__main__":
    sys.exit(main())
