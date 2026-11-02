"""Scratch directories that clean up after themselves, as the Rust `plumcraft-scratch` crate: one
root per process, `$TMPDIR/plumcraft-test-<pid>`, and the first use sweeps the roots (and host
socket directories, `plumcraft-<pid>-<n>`, `plumcraft-http-<pid>-<n>`) of processes that have
exited. A process's own files stay until a later run sweeps them."""

import functools
import os
import re
import shutil
import tempfile

_OWNER = re.compile(r"plumcraft-(?:test-(\d+)|(?:http-)?(\d+)-\d+)")


def owner(name: str) -> int | None:
    m = _OWNER.fullmatch(name)
    if not m:
        return None
    pid = int(m.group(1) or m.group(2))
    return pid if pid <= 0xFFFFFFFF else None


def alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except OSError:
        return True
    return True


def sweep(tmp: str) -> int:
    """Removes the scratch directories in `tmp` whose process has exited; how many."""
    removed = 0
    for e in os.scandir(tmp):
        pid = owner(e.name)
        if pid is None or pid == os.getpid() or alive(pid) or not e.is_dir(follow_symlinks=False):
            continue
        shutil.rmtree(e.path, ignore_errors=True)
        removed += not os.path.exists(e.path)
    return removed


@functools.cache
def root() -> str:
    tmp = tempfile.gettempdir()
    sweep(tmp)
    r = os.path.join(tmp, f"plumcraft-test-{os.getpid()}")
    os.makedirs(r, exist_ok=True)
    return r


def mkdtemp(prefix: str) -> str:
    """A fresh directory under this process's root."""
    return tempfile.mkdtemp(prefix=prefix, dir=root())
