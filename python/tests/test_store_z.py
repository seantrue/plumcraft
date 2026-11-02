"""Compressed raw blobs and memo records in the store, as `crates/store/src/lib.rs`: a large
compressible raw blob is stored deflated under `plumz` (its identity still the MD5 of the
uncompressed bytes, with a `commonkit` locator only), anything else as it is; records round-trip;
damage and bad keys are misses, never errors."""

import json
import os
import random

import numpy as np
import scratch

from plumcraft_engine import data


def root() -> str:
    return scratch.mkdtemp("z-")


def test_a_large_compressible_blob_is_stored_compressed_and_read_back_whole():
    r = root()
    plane = data.Plane(r, shm=True, max_inline=1024)
    text = json.dumps([{"x": i, "name": "node"} for i in range(20_000)])
    ref = plane.text_ref(text)
    handle = data.raw(text.encode())
    assert ref["handle"] == handle, "the identity is the uncompressed bytes'"
    assert ref["at"] == [{"kind": "commonkit", "key": data.z_key_for(handle)}], "compressed: no path locator"
    store = data.open_store(r)
    assert not os.path.exists(store.path_for(data.key_for(handle))), "not also stored plain"
    assert os.path.getsize(store.path_for(data.z_key_for(handle))) * 5 < len(text)
    assert data.read_blob(r, handle).tobytes() == text.encode()
    assert plane.text_ref(text)["at"] == ref["at"], "found again, not rewritten"


def test_incompressible_or_small_blobs_are_stored_as_they_are():
    r = root()
    plane = data.Plane(r, shm=True, max_inline=1024)
    noise = random.Random(7).randbytes(200_000)
    ref = plane.blob_ref(data.Blob(handle=data.raw(noise), meta={"media": "application/octet-stream", "size": len(noise)}, data=noise))
    assert [loc["kind"] for loc in ref["at"]] == ["commonkit", "path"]
    with open(ref["at"][1]["path"], "rb") as f:
        assert f.read()[ref["at"][1]["offset"] :] == noise
    assert data.compress(noise) is None and data.compress(b"x" * 1000) is None
    assert data.put_raw(r, data.raw(b"a,b\n1,2\n" * 20_000), b"a,b\n1,2\n" * 20_000)
    assert os.path.isfile(data.open_store(r).path_for(data.z_key_for(data.raw(b"a,b\n1,2\n" * 20_000)))), "uploads are compressed too"


def test_a_damaged_compressed_blob_is_a_miss():
    r = root()
    text = "the corgi " * 20_000
    data.Plane(r, shm=True, max_inline=1024).text_ref(text)
    path = data.open_store(r).path_for(data.z_key_for(data.raw(text.encode())))
    a = np.load(path)
    np.save(path, a[: len(a) // 2])  # a valid segment holding half the stream
    assert data.read_blob(r, data.raw(text.encode())) is None
    assert data.inflate(b"not a zlib stream") is None


def test_memo_records_round_trip_and_bad_plan_keys_are_refused():
    r = root()
    key = "memo:md5:0123456789abcdef0123456789abcdef"
    assert data.read_record(r, key) is None
    assert data.put_record(r, key, b'{"v": 1}')
    assert data.read_record(r, key) == b'{"v": 1}'
    assert data.put_record(r, key, b'{"v": 2}'), "rewritten when the node is recomputed"
    assert data.read_record(r, key) == b'{"v": 2}'
    for bad in ["memo:md5:../../etc/passwd", "memo:md5:0123456789ABCDEF0123456789ABCDEF", "md5:raw:0123456789abcdef0123456789abcdef", "", None, 7]:
        assert data.memo_key_for(bad) is None and not data.put_record(r, bad, b"{}") and data.read_record(r, bad) is None
