"""`<engine> register` and `<engine> unregister`: offer an installed engine to PlumCraft.

An engine installed as a package (`uv tool install`, `pipx`, `pip`) has a console script but no
manifest PlumCraft can find. `register` writes one into `~/.config/plumcraft/engines/<id>.toml`
(the folder every editor reads, `docs/interface.md` 16.6) that starts the engine with the
interpreter it was installed with, so it works wherever the script lives. The demo is the one the
package carries (`<package>/examples/demo.json`, which the generated project's wheel includes),
when it has one. `unregister` removes a manifest `register` wrote, and nothing else.

A development checkout is offered by linking its own manifest there (`scripts/install`). That
link is left alone unless `--force`, so registering an installed copy never silently replaces the
checkout someone is working on.
"""

from __future__ import annotations

import importlib.util
import json
import os
import sys
from importlib import metadata, resources
from pathlib import Path

MARK = "# Written by `{script} register`"

USAGE = """usage: {script} register [--force] [--print] [--folder DIR]
       {script} unregister [--folder DIR]

  register     offer this engine in every PlumCraft editor: write
               ~/.config/plumcraft/engines/{id}.toml, which starts it with this Python
    --force    replace a manifest this command didn't write (a checkout's scripts/install link)
    --print    print the manifest instead of writing it
    --folder   another engines folder
  unregister   remove the manifest register wrote
"""


def folder(args: list[str]) -> Path:
    if "--folder" in args and args.index("--folder") + 1 < len(args):
        return Path(args[args.index("--folder") + 1]).expanduser()
    return Path.home() / ".config" / "plumcraft" / "engines"


def _toml(s: str) -> str:
    # a TOML basic string: JSON's escapes are TOML's
    return json.dumps(s)


def _package(cls: type) -> str:
    return cls.__module__.split(".")[0]


def _demo(package: str) -> Path | None:
    try:
        demo = resources.files(package) / "examples" / "demo.json"
    except (ModuleNotFoundError, TypeError):
        return None
    path = Path(str(demo))
    return path if path.is_file() else None


def _sdk_version() -> str:
    try:
        return metadata.version("plumcraft-engine")
    except metadata.PackageNotFoundError:
        return "?"


def _command(cls: type) -> list[str]:
    """How the manifest starts engine class `cls`: this Python with its package's `__main__`,
    else the program this was run as. Every path in it is absolute, so the manifest doesn't depend
    on the folder it is written to (`docs/interface.md` 16.6)."""
    package = _package(cls)
    if importlib.util.find_spec(f"{package}.__main__") is not None:
        return [sys.executable, "-m", package]
    return [str(Path(sys.argv[0]).resolve())]


def _unstartable(command: list[str]) -> str | None:
    """Why a manifest running `command` wouldn't start, when it certainly wouldn't: its program
    isn't a file this user may run (a script run as `python path/to/script.py register`)."""
    program = Path(command[0])
    if program.is_file() and os.access(program, os.X_OK):
        return None
    return (
        f"the manifest would start {program}, which isn't a program this user can run; "
        "run register from the engine's installed command, or from its project (`uv run <command> register`)"
    )


#: What a cache folder's `CACHEDIR.TAG` starts with (the Cache Directory Tagging spec, which uv
#: follows: it writes one at the root of its cache).
CACHEDIR_SIGNATURE = "Signature: 8a477f597d28d172789f06886806bc55"


def _bucket(name: str) -> bool:
    """A uv cache bucket's name (`archive-v0`, `builds-v0`, `environments-v2`), or one of its
    temporary folders (`.tmpXXXX`)."""
    stem, _, version = name.rpartition("-v")
    return name.startswith(".tmp") or (stem.replace("-", "").isalpha() and stem.islower() and version.isdigit())


def uv_cache(program: str) -> tuple[Path, bool] | None:
    """Where `program` is when it is inside uv's cache: the cache's root, and whether it is in a
    temporary environment there (a `.tmp…` folder or `builds-v0`, as `uv run --with` makes and
    removes when it ends). `None` when it isn't (a `uv tool install`, a project's `.venv`).

    uv's cache is found by what it is, not by where uv says it is: a folder holding a
    `CACHEDIR.TAG` whose child on the way down is a uv bucket. The folder holding the program
    is taken as a real path (not the program, which in a virtual environment links to the base
    interpreter), so a cache reached through a link is found too."""
    try:
        here = Path(program).parent.resolve()
    except (OSError, RuntimeError, ValueError):
        return None
    below: list[str] = []
    for d in [here, *here.parents]:
        tag = d / "CACHEDIR.TAG"
        try:
            tagged = tag.is_file() and tag.read_text("utf-8", errors="replace").startswith(CACHEDIR_SIGNATURE)
        except OSError:
            tagged = False
        if tagged and below and _bucket(below[-1]):
            return d, below[-1] == "builds-v0" or any(p.startswith(".tmp") for p in below)
        below.append(d.name)
    return None


def manifest(cls: type, script: str) -> str:
    """The manifest for engine class `cls` as installed here."""
    package = _package(cls)
    command = _command(cls)
    name = getattr(cls, "name", None) or cls.id
    lines = [
        f"{MARK.format(script=script)} (plumcraft-engine {_sdk_version()}): how PlumCraft starts the installed {name}.",
        f"# `{script} unregister` removes it; run `{script} register` again after reinstalling it elsewhere.",
        f"id = {_toml(cls.id)}",
        f"name = {_toml(name)}",
        'mode = "process"',
        f"command = [{', '.join(_toml(c) for c in command)}]",
        'restart = "on-crash"',
        "max_restarts = 5",
        f"hint = {_toml(f'reinstall it, then run `{script} register` again')}",
    ]
    demo = _demo(package)
    if demo is not None:
        lines.append(f"demo = {_toml(str(demo))}")
    return "\n".join(lines) + "\n"


def _ours(path: Path) -> bool:
    try:
        return not path.is_symlink() and path.read_text("utf-8").startswith("# Written by `") and " register`" in path.read_text("utf-8").splitlines()[0]
    except (OSError, IndexError):
        return False


def register(cls: type, args: list[str], script: str) -> int:
    text = manifest(cls, script)
    if "--print" in args:
        sys.stdout.write(text)
        return 0
    why = _unstartable(_command(cls))
    if why is not None:
        # a manifest that can't start is worse than none: the editor would offer a dead engine
        print(f"{script}: not registered: {why}", file=sys.stderr)
        return 1
    cached = uv_cache(_command(cls)[0])
    if cached is not None and cached[1]:
        print(
            f"{script}: not registered: this Python is a temporary environment in uv's cache ({cached[0]}), as `uv run --with` makes,"
            " which uv removes when that command ends, so the manifest could never start it."
            " Install the engine (`uv tool install <package>`) and run register from the command that installs",
            file=sys.stderr,
        )
        return 1
    if cached is not None:
        print(
            f"{script}: warning: this Python is in uv's cache ({cached[0]}), as `uvx` and `uv tool run` make: a cache clean"
            " (`uv cache clean`, `uv cache prune`) removes it, and the engine stops starting."
            " For an install that lasts, `uv tool install <package>` and run register from the command that installs",
            file=sys.stderr,
        )
    target = folder(args) / f"{cls.id}.toml"
    if (target.exists() or target.is_symlink()) and not _ours(target) and "--force" not in args:
        what = f"a link to {os.path.realpath(target)} (a checkout's scripts/install)" if target.is_symlink() else "a manifest this command didn't write"
        print(f"{script}: {target} is {what}; leaving it. --force replaces it", file=sys.stderr)
        return 1
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.is_symlink():
        target.unlink()
    tmp = target.with_suffix(".toml.tmp")
    tmp.write_text(text, "utf-8", newline="\n")
    tmp.replace(target)
    print(f"registered {cls.id}: {target}\nA running editor offers it after Engine › Reload Engine List.")
    return 0


def unregister(cls: type, args: list[str], script: str) -> int:
    target = folder(args) / f"{cls.id}.toml"
    if not (target.exists() or target.is_symlink()):
        print(f"{script}: {cls.id} isn't registered ({target})", file=sys.stderr)
        return 1
    if not _ours(target):
        print(f"{script}: {target} wasn't written by `{script} register`; leaving it (a checkout's link goes with its scripts/uninstall)", file=sys.stderr)
        return 1
    target.unlink()
    print(f"unregistered {cls.id}")
    return 0


def main(cls: type, args: list[str]) -> int | None:
    """Handle `register`, `unregister` and `--help`; `None` for anything else (serve)."""
    script = Path(sys.argv[0]).name or cls.id
    if args[:1] == ["register"]:
        return register(cls, args[1:], script)
    if args[:1] == ["unregister"]:
        return unregister(cls, args[1:], script)
    if args[:1] in (["-h"], ["--help"], ["help"]):
        sys.stdout.write(USAGE.format(script=script, id=cls.id))
        return 0
    return None
