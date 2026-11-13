"""AUDIO and the other core types in the Python SDK: `Text`, `Table`, `Image` and `Audio` declare
exactly the Rust SDK's wire types; an `AudioValue` crosses as the Rust SDK's audio does (an
`nd1` blob of (frames, channels) float32 with `Form::Audio`), shows as `peaks` laid out as the
Rust SDK lays them out, comes back from a binding with its sample rate, and persists."""

import base64
import time

import numpy as np
import pytest
import scratch
from commonkit import digest as ck_digest

from plumcraft_engine import Audio, AudioValue, Engine, Image, Json, Table, Text, data, node, persist
from plumcraft_engine.audio import PEAKS_BYTES, peaks_array
from plumcraft_engine.host import Host

# plumcraft_proto::catalogue::core_types::{text, table, image, audio}()
CORE = {
    "TEXT": {"id": "TEXT", "media": "text", "color": "#c9a227", "thumb": True},
    "TABLE": {"id": "TABLE", "media": "table", "color": "#4f86c6"},
    "IMAGE": {"id": "IMAGE", "media": "image", "color": "#2563eb", "thumb": True},
    "AUDIO": {"id": "AUDIO", "media": "audio", "color": "#2fa39a", "thumb": True},
    "JSON": {"id": "JSON", "media": "tree", "color": "#64748b"},
}


class Sound(Engine):
    id = "sound"
    name = "Sound"
    version = "1"

    @node
    def tone(self, hz: float = 440.0, seconds: float = 0.01, rate: int = 8000) -> Audio:
        t = np.arange(int(seconds * rate)) / rate
        return AudioValue(np.stack([np.sin(2 * np.pi * hz * t), 0.5 * np.sin(2 * np.pi * hz * t)], axis=1), rate)

    @node
    def gain(self, audio: Audio, db: float = -6.0) -> Audio:
        return AudioValue(audio.samples * 10 ** (db / 20), audio.sample_rate)

    @node
    def words(self, t: Text, table: Table | None = None, picture: Image | None = None) -> Text:
        return t

    @node
    def settings(self, big: int = 0) -> Json:
        return {"name": "corgi", "padding": "x" * big}


def reply(h: Host, run_id: str, graph: dict, targets: list[str]) -> tuple[dict, list]:
    h.run({"v": 2, "op": "run", "run_id": run_id, "generation": 1, "graph": {"kind": "inline", "graph": graph}, "targets": targets})
    deadline, since, events = time.monotonic() + 10, 0, []
    while time.monotonic() < deadline:
        p = h.poll(since, 200, run_id)
        since = p["last_seq"]
        events += p["events"]
        for e in p["events"]:
            if e["kind"] == "run.reply":
                return e["body"], events
    raise AssertionError(f"no reply for {run_id}")


def test_the_core_types_are_declared_exactly():
    declared = {w["id"]: w for w in Sound().wire_types}
    for id, want in CORE.items():
        assert declared[id] == want, id


def test_large_json_crosses_as_a_blob():
    h = Host(Sound)
    try:
        assert h.wait_ready(5)["state"] == "connected"
        small, _ = reply(h, "s", {"nodes": {"j": {"kind": "sound.settings"}}, "wires": []}, ["j"])
        assert small["nodes"]["j"]["outputs"]["json"] == {"kind": "json", "value": {"name": "corgi", "padding": ""}}
        large, _ = reply(h, "l", {"nodes": {"j": {"kind": "sound.settings", "values": {"big": 200_000}}}, "wires": []}, ["j"])
        out = large["nodes"]["j"]["outputs"]["json"]
        assert out["kind"] == "blob" and out["ref"]["meta"]["form"] == {"kind": "json"} and out["ref"]["handle"].startswith(f"{ck_digest.current()}:raw:")
    finally:
        h.shutdown()


def test_an_audio_value_checks_itself():
    a = AudioValue([0.0, 0.5, -0.5], 8000)
    assert (a.frames, a.channels, a.samples.dtype.str) == (3, 1, "<f4")
    assert a.summary() == "audio 8000 Hz, 1 ch, 0.00 s"
    for bad in [(np.zeros((2, 2, 2)), 8000), ([0.0], 0), ([0.0], 8000.0), ([0.0], True)]:
        with pytest.raises(ValueError):
            AudioValue(*bad)


def test_peaks_are_the_rust_sdks():
    # data::peaks: bins = min(PEAKS_BYTES / (channels * 8), frames), each the min and max of its frames
    a = AudioValue(np.array([[0.0, 1.0], [2.0, -1.0], [-3.0, 0.5], [np.nan, 4.0]]), 100)
    p = peaks_array(a)
    assert p.shape == (1, 2, 4, 2)
    assert p[0, 0, :, 0].tolist()[:3] == [0.0, 2.0, -3.0]
    long = AudioValue(np.zeros(100_000), 100)
    assert peaks_array(long).shape == (1, 1, PEAKS_BYTES // 8, 2)
    assert np.isnan(p[0, 0, 3]).all(), "a bin of only NaN"


def test_audio_crosses_as_the_rust_sdks_audio_and_binds_back():
    h = Host(Sound)
    try:
        assert h.wait_ready(5)["state"] == "connected"
        wire = {"from": {"node": "t", "port": "audio"}, "to": {"node": "g", "port": "audio"}}
        graph = {"nodes": {"t": {"kind": "sound.tone"}, "g": {"kind": "sound.gain"}}, "wires": [wire]}
        body, events = reply(h, "r", graph, ["t", "g"])
        out = body["nodes"]["t"]["outputs"]["audio"]
        assert out["kind"] == "blob"
        ref = out["ref"]
        assert ref["meta"]["form"] == {"kind": "audio", "sample_rate": 8000, "channels": 2, "frames": 80, "dtype": "<f4"}
        assert ref["handle"].startswith(f"{ck_digest.current()}:nd1:")
        thumbs = [e["body"]["thumb"] for e in events if e["kind"] == "node.status" and e["body"].get("thumb")]
        assert thumbs and thumbs[0]["kind"] == "peaks" and thumbs[0]["channels"] == 2
        # bound back in as another engine's audio: an AudioValue at its rate
        silence = np.zeros((4, 1), "<f4")
        form = {"kind": "audio", "sample_rate": 44100, "channels": 1, "frames": 4, "dtype": "<f4"}
        inline = [{"kind": "inline", "b64": base64.b64encode(silence.tobytes()).decode()}]
        bound = {"handle": data.nd1(silence), "meta": {"media": "application/x-npy", "size": 16, "form": form}, "at": inline}
        graph = {"kind": "inline", "graph": {"nodes": {"g": {"kind": "sound.gain"}}, "wires": []}}
        h.run({"v": 2, "op": "run", "run_id": "b", "generation": 1, "graph": graph, "targets": ["g"], "inputs": {"g.audio": {"kind": "blob", "ref": bound}}})
        body, _ = _last(h, "b")
        assert body["nodes"]["g"]["outputs"]["audio"]["ref"]["meta"]["form"]["sample_rate"] == 44100
    finally:
        h.shutdown()


def _last(h: Host, run_id: str) -> tuple[dict, list]:
    deadline, since = time.monotonic() + 10, 0
    while time.monotonic() < deadline:
        p = h.poll(since, 200, run_id)
        since = p["last_seq"]
        for e in p["events"]:
            if e["kind"] == "run.reply":
                return e["body"], []
    raise AssertionError(f"no reply for {run_id}")


def test_audio_persists():
    root = scratch.mkdtemp("audio-")
    a = AudioValue(np.linspace(-1, 1, 64).reshape(32, 2), 22050)
    key = f"memo:{ck_digest.current()}:" + "a" * len(ck_digest.raw(b""))
    assert persist.save(root, key, {"audio": a}, 1e6)
    back = persist.load(root, key)[0]["audio"]
    assert isinstance(back, AudioValue) and back.sample_rate == 22050 and np.array_equal(back.samples, a.samples)
