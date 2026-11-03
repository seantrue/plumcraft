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


def test_anything_else_is_served():
    assert register.main(TextEngine, []) is None
    assert register.main(TextEngine, ["--listen", "127.0.0.1:0"]) is None
