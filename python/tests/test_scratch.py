"""The scratch sweep (tests/scratch.py), as the Rust crate's: only scratch names of exited
processes go; the living, the unrelated and plain files stay."""

import os
import subprocess

import pytest
import scratch


def test_owners():
    assert scratch.owner("plumcraft-test-123") == 123
    assert scratch.owner("plumcraft-123-4") == 123
    assert scratch.owner("plumcraft-http-123-4") == 123
    unrelated = ["plumcraft-test-", "plumcraft-test-12x", "plumcraft-123", "plumcraft-e2-123-hunt", "plumcraft-control.token"]
    unrelated += ["plumcraft-screenshots", "plumcraft-http-123", "other-123-4", "plumcraft-test-99999999999"]
    for name in unrelated:
        assert scratch.owner(name) is None, name


@pytest.mark.skipif(os.name == "nt", reason="nothing is swept on Windows: os.kill(pid, 0) would end the process (scratch.alive)")
def test_sweeps_the_dead_and_keeps_the_living():
    tmp = scratch.mkdtemp("sweep-")
    p = subprocess.Popen(["true"])
    p.wait()
    dead, me, parent = p.pid, os.getpid(), os.getppid()
    gone = [f"plumcraft-test-{dead}", f"plumcraft-{dead}-0", f"plumcraft-http-{dead}-3"]
    kept = [f"plumcraft-test-{me}", f"plumcraft-test-{parent}", f"plumcraft-{parent}-1", "plumcraft-screenshots"]
    for n in gone + kept:
        os.makedirs(os.path.join(tmp, n, "inner"))
    with open(os.path.join(tmp, f"plumcraft-{dead}-9"), "w") as f:
        f.write("x")
    assert scratch.sweep(tmp) == len(gone)
    assert not any(os.path.exists(os.path.join(tmp, n)) for n in gone)
    assert all(os.path.exists(os.path.join(tmp, n)) for n in kept)
    assert os.path.exists(os.path.join(tmp, f"plumcraft-{dead}-9"))


def test_root_is_per_process():
    r = scratch.root()
    assert os.path.basename(r) == f"plumcraft-test-{os.getpid()}" and os.path.isdir(r)
    assert scratch.mkdtemp("x-").startswith(r)
