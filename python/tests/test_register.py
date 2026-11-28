"""`<engine> register` and `unregister` (plumcraft_engine.register): the manifest they write
starts the installed engine with this Python, they leave a checkout's link alone, and unregister
removes only what register wrote."""

import os
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest
import scratch

from plumcraft_engine import register
from plumcraft_text import TextEngine


@pytest.fixture
def engines(monkeypatch) -> Path:
    monkeypatch.setattr(sys, "argv", ["text-engine"])
    return Path(scratch.mkdtemp("engines-")) / "engines"


def run(*args: str, folder: Path) -> int | None:
    return register.main(TextEngine, [*args, "--folder", str(folder)])


def test_register_writes_a_manifest_that_starts_this_python(engines, capsys):
    assert run("register", folder=engines) == 0
    m = tomllib.loads((engines / "text.toml").read_text())
    assert (m["id"], m["mode"], m["command"]) == ("text", "process", [sys.executable, "-m", "plumcraft_text"])
    assert "Reload Engine List" in capsys.readouterr().out
    # again: its own manifest is replaced
    assert run("register", folder=engines) == 0
    # the command it names really serves the engine (asked for help, it answers)
    out = subprocess.run([*m["command"], "--help"], capture_output=True, text=True, check=True).stdout
    assert "register" in out and "unregister" in out


def test_print_writes_nothing(engines, capsys):
    assert run("register", "--print", folder=engines) == 0
    assert 'id = "text"' in capsys.readouterr().out
    assert not engines.exists()


def test_a_checkouts_link_is_left_unless_forced(engines, capsys):
    engines.mkdir(parents=True)
    checkout = engines.parent / "plumcraft-engine.toml"
    checkout.write_text('id = "text"\n')
    os.symlink(checkout, engines / "text.toml")
    assert run("register", folder=engines) == 1
    assert "scripts/install" in capsys.readouterr().err
    assert (engines / "text.toml").is_symlink()
    assert run("unregister", folder=engines) == 1, "not register's to remove"
    assert (engines / "text.toml").is_symlink()
    assert run("register", "--force", folder=engines) == 0
    assert not (engines / "text.toml").is_symlink()
    assert checkout.read_text() == 'id = "text"\n', "the checkout's own manifest is untouched"


def test_unregister_removes_only_what_register_wrote(engines, capsys):
    assert run("unregister", folder=engines) == 1
    assert "isn't registered" in capsys.readouterr().err
    engines.mkdir(parents=True)
    (engines / "text.toml").write_text('id = "text"\n')
    assert run("unregister", folder=engines) == 1
    assert (engines / "text.toml").exists()
    (engines / "text.toml").unlink()
    run("register", folder=engines)
    assert run("unregister", folder=engines) == 0
    assert not (engines / "text.toml").exists()


def test_a_manifest_that_cannot_start_is_not_written(engines, capsys, monkeypatch):
    # an engine with no `__main__`, run as a script that isn't executable (`python engine.py register`)
    script = Path(scratch.mkdtemp("engine-script-")) / "engine.py"
    script.write_text("# not executable\n")
    script.chmod(0o644)
    monkeypatch.setattr(sys, "argv", [str(script)])
    monkeypatch.setattr(register.importlib.util, "find_spec", lambda name: None)
    assert run("register", folder=engines) == 1
    err = capsys.readouterr().err
    assert "not registered" in err and str(script.resolve()) in err, err
    assert not engines.exists(), "nothing written"
    # an executable program is registered as before
    script.chmod(0o755)
    assert run("register", folder=engines) == 0
    assert tomllib.loads((engines / "text.toml").read_text())["command"] == [str(script.resolve())]


def _fake_uv_cache(root: Path) -> Path:
    """A folder laid out as uv's cache: its CACHEDIR.TAG, and the buckets register looks for."""
    (root / "uv").mkdir(parents=True)
    (root / "uv" / "CACHEDIR.TAG").write_text(register.CACHEDIR_SIGNATURE + "\n# This file is a cache directory tag created by uv.\n")
    return root / "uv"


def _python_in(env: Path) -> Path:
    """An executable stand-in for an environment's interpreter, at `env/bin/python`."""
    (env / "bin").mkdir(parents=True)
    python = env / "bin" / "python"
    python.write_text("#!/bin/sh\n")
    python.chmod(0o755)
    return python


def test_uv_cache_is_found_by_its_tag_and_bucket():
    root = Path(scratch.mkdtemp("uv-cache-"))
    cache = _fake_uv_cache(root)
    assert register.uv_cache(str(_python_in(cache / "archive-v0" / "AbC123"))) == (cache.resolve(), False), "uvx: lasting until a clean"
    assert register.uv_cache(str(_python_in(cache / "environments-v2" / "w-1a2b"))) == (cache.resolve(), False)
    assert register.uv_cache(str(_python_in(cache / "builds-v0" / ".tmpQx9"))) == (cache.resolve(), True), "uv run --with: removed at its end"
    assert register.uv_cache(str(_python_in(cache / ".tmpZz"))) == (cache.resolve(), True)
    # a tool install, a project's .venv, a tag that isn't a cache's, a folder not a bucket: not uv's cache
    assert register.uv_cache(str(_python_in(root / "tools" / "w-engine"))) is None
    assert register.uv_cache(str(_python_in(root / "project" / ".venv"))) is None
    (root / "other").mkdir()
    (root / "other" / "CACHEDIR.TAG").write_text("not a signature\n")
    assert register.uv_cache(str(_python_in(root / "other" / "archive-v0" / "x"))) is None
    assert register.uv_cache(str(_python_in(cache / "mine" / "x"))) is None
    # reached through a link: the real folder is the cache's
    os.symlink(cache, root / "linked")
    assert register.uv_cache(str(root / "linked" / "archive-v0" / "AbC123" / "bin" / "python")) == (cache.resolve(), False)
    assert register.uv_cache("") is None and register.uv_cache("\0") is None


def test_register_from_uvs_cache_warns_and_from_a_temporary_one_refuses(engines, capsys, monkeypatch):
    cache = _fake_uv_cache(Path(scratch.mkdtemp("uv-cache-reg-")))
    lasting = _python_in(cache / "archive-v0" / "AbC123")
    monkeypatch.setattr(sys, "executable", str(lasting))
    assert run("register", folder=engines) == 0, "still written"
    err = capsys.readouterr().err
    assert "warning: this Python is in uv's cache" in err and "uv cache clean" in err, err
    assert tomllib.loads((engines / "text.toml").read_text())["command"][0] == str(lasting)
    (engines / "text.toml").unlink()
    monkeypatch.setattr(sys, "executable", str(_python_in(cache / "builds-v0" / ".tmpQx9")))
    assert run("register", folder=engines) == 1
    err = capsys.readouterr().err
    assert "not registered" in err and "temporary environment" in err, err
    assert not (engines / "text.toml").exists()
    # this test's own Python is no uv cache's: no warning
    monkeypatch.undo()
    monkeypatch.setattr(sys, "argv", ["text-engine"])
    if register.uv_cache(sys.executable) is None:
        assert run("register", folder=engines) == 0
        assert "uv's cache" not in capsys.readouterr().err


def test_print_from_uvs_cache_warns_too(engines, capsys, monkeypatch):
    """Decision 18: `register --print` writes nothing, but says on stderr what register would
    about uv's cache; the manifest alone is on stdout."""
    cache = _fake_uv_cache(Path(scratch.mkdtemp("uv-cache-print-")))
    monkeypatch.setattr(sys, "executable", str(_python_in(cache / "archive-v0" / "AbC123")))
    assert run("register", "--print", folder=engines) == 0
    out, err = capsys.readouterr()
    assert out.startswith("# Written by `") and "uv's cache" not in out, out
    assert "warning: this Python is in uv's cache" in err and "uv cache clean" in err, err
    monkeypatch.setattr(sys, "executable", str(_python_in(cache / "builds-v0" / ".tmpQx9")))
    assert run("register", "--print", folder=engines) == 0
    err = capsys.readouterr().err
    assert "temporary environment" in err and "could never start" in err, err
    assert not (engines / "text.toml").exists(), "--print wrote nothing"


def test_anything_else_is_served():
    assert register.main(TextEngine, []) is None
    assert register.main(TextEngine, ["--listen", "127.0.0.1:0"]) is None


def test_paths_a_person_reads_lose_windows_long_path_prefix():
    """B95 in the Python SDK (decision 8A): what register prints shows a path without its
    long-path prefix, as `plumcraft_proto::programs::shown`; the same string handling on every
    platform (the Windows test VM was down, so this ran on the Mac only)."""
    cases = {
        r"\\?\C:\Users\a\cache": r"C:\Users\a\cache",
        r"\\?\UNC\server\share\x": r"\\server\share\x",
        r"\\?\GLOBALROOT\Device\x": r"\\?\GLOBALROOT\Device\x",
        r"\\?\C": r"\\?\C",
        r"C:\plain": r"C:\plain",
        "/usr/local/bin": "/usr/local/bin",
        "": "",
    }
    for given, want in cases.items():
        assert register.shown(given) == want, given
    assert register.shown(Path("/a/b")) == str(Path("/a/b"))


def test_register_prints_a_cache_root_without_the_prefix(engines, capsys, monkeypatch):
    root = r"\\?\C:\Users\a\AppData\Local\uv\cache"
    monkeypatch.setattr(register, "uv_cache", lambda _p: (root, False))
    assert run("register", folder=engines) == 0
    err = capsys.readouterr().err
    assert r"(C:\Users\a\AppData\Local\uv\cache)" in err and "\\\\?\\" not in err, err
    monkeypatch.setattr(register, "uv_cache", lambda _p: (root, True))
    assert run("register", folder=engines) == 1
    assert "\\\\?\\" not in capsys.readouterr().err


def test_the_demo_is_the_packages_plum_or_an_older_json(monkeypatch):
    # FR8: a package generated now carries `examples/demo.plum`; one generated before, `demo.json`
    root = Path(scratch.mkdtemp("demo-pkg-"))
    for name, files in (("fr8_new_pkg", ["demo.plum", "demo.json"]), ("fr8_old_pkg", ["demo.json"]), ("fr8_no_demo", [])):
        (root / name / "examples").mkdir(parents=True)
        (root / name / "__init__.py").write_text("")
        for f in files:
            (root / name / "examples" / f).write_text('{"nodes": {}}')
    monkeypatch.syspath_prepend(str(root))
    assert register._demo("fr8_new_pkg").name == "demo.plum"
    assert register._demo("fr8_old_pkg").name == "demo.json"
    assert register._demo("fr8_no_demo") is None
