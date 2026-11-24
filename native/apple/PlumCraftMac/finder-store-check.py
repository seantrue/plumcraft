"""B149, the person check's launch as the Finder makes it with no store variable: an engine that
stores through commonkit (the array engine) runs a picture, and the node's preview is read the
way the canvas reads it (`media.get {handle, want: "rgba"}`), which must come back ready, and the
value's bytes must be in the app's own store (under scratch), not in commonkit's default one.

    finder-store-check.py STATUS TOKEN SCRATCH STORE DEFAULT_STORE [GRAPH NODE]

GRAPH (a graph file) and NODE (the node whose preview is read) default to an Array Mandelbrot.
PlumCraft's own in-process engines are given the core's store, so they pass with or without the
fix; an application's engine that stores through commonkit itself (as an application built on
the Mac shell does) is what failed, and is checked by giving its graph here.

Prints `PERSON PASS|FAIL | step | what it saw` lines; exits non-zero when one failed."""

import json
import os
import socket
import sys
import time

GRAPH = {"version": 1, "nodes": {"m": {"kind": "array.mandelbrot", "pos": [0, 0]}}, "wires": []}


def wait(f, seconds, every=0.2):
    end = time.time() + seconds
    while time.time() < end:
        v = f()
        if v:
            return v
        time.sleep(every)
    return None


def files(root):
    out = []
    for d, _, fs in os.walk(root):
        out += [os.path.join(d, f) for f in fs if not f.startswith(".")]
    return out


def main():
    status, token, scratch, store, default = sys.argv[1:6]

    def st():
        try:
            return json.load(open(status))
        except (OSError, ValueError):
            return {}
    ready = wait(lambda: (lambda s: s if s.get("port") and s.get("windows") and s["windows"][0].get("ready") else None)(st()), 60)
    if not ready:
        print(f"PERSON FAIL | B149: the app launched with no store variable | no ready window in 60 s: {st()}")
        return 1
    doc = ready["windows"][0]["doc"]
    s = socket.create_connection(("127.0.0.1", ready["port"]), timeout=120)
    f = s.makefile("rw")
    n = [0]

    def call(m, p=None):
        n[0] += 1
        f.write(json.dumps({"id": n[0], "method": m, "params": p or {}, "doc": doc}) + "\n")
        f.flush()
        return json.loads(f.readline())
    if not call("auth", {"token": open(token).read().strip()}).get("ok"):
        print("PERSON FAIL | B149: the control port took the token | refused")
        return 1
    if len(sys.argv) > 7:
        path, node_id = sys.argv[6], sys.argv[7]
    else:
        path, node_id = os.path.join(scratch, "b149-picture.json"), "m"
        json.dump(GRAPH, open(path, "w"))
    opened = call("command.run", {"command": "graph.open", "params": {"path": path, "discard": True}})
    call("command.run", {"command": "run.start"})
    ran = call("ui.waitRun", {"timeout": 120}).get("ok")

    def preview():
        for node in call("scene.get").get("result", {}).get("nodes", []):
            if node.get("id") == node_id:
                p = node.get("preview") or {}
                return p if p.get("handle") else None
        return None
    p = wait(preview, 30, every=0.3)
    note = {}
    if p:
        h = p["handle"]
        # the preview's reference: its content handle (a DataRef as JSON, or the handle itself)
        handle = h if isinstance(h, str) else h.get("handle", "")
        if handle.startswith("{"):
            handle = json.loads(handle).get("handle", "")

        def got():
            r = call("media.get", {"handle": handle, "want": "rgba"}).get("result") or {}
            return r if r.get("status") not in (None, "pending") else None
        note = wait(got, 30, every=0.3) or {"status": "pending after 30 s"}
    mine, theirs = files(store), files(default)
    ok = bool(opened.get("ok") and ran and p and note.get("status") == "ready" and note.get("w") and mine and not theirs)
    print(f"PERSON {'PASS' if ok else 'FAIL'} | B149: launched as the Finder does with no store variable, a picture an engine "
          f"stores through commonkit shows its preview | ran {ran}; preview {bool(p)}; media.get {json.dumps(note)[:300]}; "
          f"the app's store {len(mine)} file(s); commonkit's default store {len(theirs)} file(s)")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
