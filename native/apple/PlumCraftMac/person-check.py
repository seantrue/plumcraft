#!/usr/bin/env python3
"""The person check's steps (person-check.sh launches the app; this drives and looks).

Every step prints `PERSON PASS | name | what it saw` or `PERSON FAIL | name | what it saw`
(`PERSON SEE | …` for what it cannot see). The app is driven where a person drives it, through
the app's own steps (`--then inbox:DIR`, PersonSteps.swift): menu items of the real menu bar
picked, a sheet's fields filled and buttons clicked, the palette and the canvas's drags. What it
sees comes from outside the app where it can (the accessibility API, person-check-ax), from the
app's own windows (the status file, the sheet's controls and picture), and from the core's
control port only to read the graph and a value. Every wait has a time limit.
"""

import argparse
import json
import os
import socket
import subprocess
import sys
import time

A = None
results = []


def say(ok, name, saw):
    word = "PASS" if ok is True else ("FAIL" if ok is False else "SEE")
    results.append(ok)
    print(f"PERSON {word} | {name} | {saw}", flush=True)
    return ok


def alive():
    try:
        os.kill(A.pid, 0)
        return True
    except OSError:
        return False


def status():
    try:
        with open(A.status) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def wait(pred, secs, every=0.1):
    """pred() until it is truthy, at most secs; its last value."""
    end = time.time() + secs
    v = None
    while time.time() < end:
        v = pred()
        if v:
            return v
        if not alive():
            return v
        time.sleep(every)
    return v


k = 0


def step(text, secs=60):
    """One step in the app's inbox; its result (or {"ok": False, "error": "timed out"})."""
    global k
    path = os.path.join(A.inbox, f"{k}.step")
    done = os.path.join(A.inbox, f"{k}.done")
    with open(path + ".tmp", "w") as f:
        f.write(text)
    os.rename(path + ".tmp", path)
    k += 1
    r = wait(lambda: os.path.exists(done), secs)
    if not r:
        return {"ok": False, "error": f"step {text!r} timed out after {secs} s"}
    with open(done) as f:
        return json.load(f)


class Port:
    """The core's control port: used only to read (the graph, a value), never to act."""

    def __init__(self):
        port = status().get("port")
        self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
        self.f = self.s.makefile("rwb")
        with open(A.token) as t:
            self.call("auth", {"token": t.read().strip()}, rid="auth")
        self.n = 0

    def call(self, method, params=None, doc=None, rid=None):
        self.n = getattr(self, "n", 0) + 1
        req = {"id": rid or self.n, "method": method, "params": params or {}}
        if doc is not None:
            req["doc"] = doc
        self.f.write((json.dumps(req) + "\n").encode())
        self.f.flush()
        r = json.loads(self.f.readline())
        if not r.get("ok"):
            raise RuntimeError(f"{method}: {r.get('error')}")
        return r.get("result")


def ax(what):
    try:
        out = subprocess.run([A.ax, what, str(A.pid)], capture_output=True, text=True, timeout=30).stdout
        return json.loads(out)
    except (subprocess.SubprocessError, ValueError, OSError) as e:
        return {"trusted": False, "error": str(e)}


def docs():
    return step("docs").get("docs", [])


def graph_windows_ax():
    r = ax("windows")
    if not r.get("trusted"):
        return None
    return [w for w in r.get("windows", []) if w.get("subrole") == "AXStandardWindow"]


def layout_ok(sheet, axsheets):
    """Every field has a label beside it, on its line, wholly shown: from the app's own controls,
    and again from accessibility when it is trusted."""
    problems = []
    cs = sheet.get("controls", [])
    fields = [c for c in cs if c["kind"] in ("field", "secret", "popup", "combo") and c.get("axLabel")]
    labels = [c for c in cs if c["kind"] == "label"]
    if not fields:
        problems.append("no labelled fields")
    for f in fields:
        want = f["axLabel"] + ":"
        mine = [lb for lb in labels if lb["text"] == want]
        if not mine:
            problems.append(f"no label {want!r} for its field")
            continue
        lb = mine[0]
        fx, fy, fw, fh = f["frame"]
        lx, ly, lw, lh = lb["frame"]
        if lx + lw > fx + 1:
            problems.append(f"{want!r} is not left of its field ({lx + lw:.0f} > {fx:.0f})")
        if abs((ly + lh / 2) - (fy + fh / 2)) > fh / 2:
            problems.append(f"{want!r} is not on its field's line (label mid {ly + lh / 2:.0f}, field mid {fy + fh / 2:.0f})")
        # the nearest label on the field's line must be its own (B73: the next field's was above it)
        line = [x for x in labels if abs((x["frame"][1] + x["frame"][3] / 2) - (fy + fh / 2)) <= fh / 2 and x["frame"][0] < fx]
        if line and min(line, key=lambda x: fx - (x["frame"][0] + x["frame"][2]))["text"] != want:
            problems.append(f"the label beside {want!r}'s field is another's")
        if lb.get("clipped"):
            problems.append(f"{want!r} is cut off")
    seen_ax = "accessibility not trusted: the app's own report only"
    if axsheets is not None:
        els = [e for s in axsheets for e in s.get("elements", [])]
        texts = [e for e in els if e.get("role") == "AXTextField" or e.get("role") == "AXPopUpButton" or e.get("role") == "AXComboBox"]
        statics = [e for e in els if e.get("role") == "AXStaticText"]
        for f in fields:
            want = f["axLabel"] + ":"
            fe = [e for e in texts if e.get("description") == f["axLabel"] or e.get("title") == f["axLabel"] or e.get("label") == f["axLabel"]]
            le = [e for e in statics if e.get("value") == want]
            if not fe or not le:
                problems.append(f"accessibility has no field {f['axLabel']!r} or no label {want!r}")
                continue
            fx, fy, fw, fh = fe[0]["frame"]
            lx, ly, lw, lh = le[0]["frame"]
            if lx + lw > fx + 1 or abs((ly + lh / 2) - (fy + fh / 2)) > fh / 2:
                problems.append(f"accessibility puts {want!r} at {le[0]['frame']} and its field at {fe[0]['frame']}")
        seen_ax = f"accessibility: {len(els)} elements in {len(axsheets)} sheet(s)"
    return problems, seen_ax


def axrun(*a):
    try:
        return json.loads(subprocess.run([A.ax, *a], capture_output=True, text=True, timeout=30).stdout)
    except (subprocess.SubprocessError, ValueError, OSError) as e:
        return {"error": str(e)}


def panel_fields(sheet):
    """A panel's own fields as accessibility reads them from outside the app (the panel runs out
    of process, so what the app's object says is not what a person sees): the save panel's name
    field and folder pop-up (a sheet), or the open panel's folder pop-ups (a window of its own)."""
    if sheet:
        els = [e for s in axrun("sheet", str(A.pid)).get("sheets", []) for e in s.get("elements", [])]
    else:
        ws = axrun("all", str(A.pid)).get("windows", [])

        # the open panel: the window that has the panel's Open button (its identifier)
        def mark(e):
            return e.get("role") == "AXButton" and e.get("identifier") == "OKButton"
        panels = [w for w in ws if any(mark(e) for e in w.get("elements", []))]
        seen_ws = [(w.get("title"), w.get("subrole"), len(w.get("elements", [])),
                    sorted({e.get("identifier") for e in w.get("elements", []) if e.get("identifier")})[:12]) for w in ws]
        if not panels:
            return {"windows": seen_ws}
        els = [e for w in panels for e in w.get("elements", [])]
    name = next((e.get("value", "") for e in els if e.get("identifier") == "saveAsNameTextField"), None)
    where = next((e.get("value", "") for e in els if e.get("identifier") == "where popup"), None)
    pops = [e.get("value") for e in els if e.get("role") == "AXPopUpButton" and e.get("value")]
    out = {"name": name, "folder": where, "folders": pops}
    if not sheet:
        out["windows"] = seen_ws
    return out


SCRATCH_PREFIX = "person-scratch-"


def may_press(seen_name, seen_folder, want, scratch, env):
    """Whether Save (or Open) may be pressed in a real panel, and why not. Never, unless
    PERSON_PRESS_SAVE=1 (set by no script: the maintainer, 2026-11-21) and the panel itself shows
    the wanted name in the wanted folder, a folder inside the scratch whose name (person-scratch-
    and eight hex digits) no folder of a person's has."""
    if env.get("PERSON_PRESS_SAVE") != "1":
        return False, "PERSON_PRESS_SAVE is not 1 (the person check never presses Save in a real panel)"
    folder = os.path.dirname(os.path.realpath(want))
    root = os.path.realpath(scratch)
    if not folder.startswith(root + os.sep):
        return False, f"{folder} is not inside the scratch {root}"
    base = os.path.basename(folder)
    tail = base[len(SCRATCH_PREFIX):]
    if not base.startswith(SCRATCH_PREFIX) or len(tail) != 8 or any(c not in "0123456789abcdef" for c in tail):
        return False, f"{base!r} is not a scratch folder's name ({SCRATCH_PREFIX}<8 hex>)"
    if seen_name is None or seen_folder is None:
        return False, "the panel's own name field or folder could not be read"
    if seen_name != os.path.basename(want):
        return False, f"the panel's name field says {seen_name!r}, not {os.path.basename(want)!r}"
    if seen_folder != base:
        return False, f"the panel's folder is {seen_folder!r}, not {base!r}"
    return True, "both fields are the scratch ones"


def self_test():
    """may_press's refusals (person-check.sh runs this before it launches anything)."""
    s = os.path.realpath(os.environ.get("TMPDIR", "/tmp"))
    good = os.path.join(s, "person-scratch-0a1b2c3d", "person.json")
    on = {"PERSON_PRESS_SAVE": "1"}
    cases = [
        (("person.json", "person-scratch-0a1b2c3d", good, s, {}), False),
        (("person.json", "person-scratch-0a1b2c3d", good, s, {"PERSON_PRESS_SAVE": "yes"}), False),
        (("person.json", "person-scratch-0a1b2c3d", good, s, on), True),
        (("Untitled", "person-scratch-0a1b2c3d", good, s, on), False),
        (("person.json", "Documents", good, s, on), False),
        ((None, "person-scratch-0a1b2c3d", good, s, on), False),
        (("person.json", None, good, s, on), False),
        (("person.json", "Documents", os.path.expanduser("~/Documents/person.json"), s, on), False),
        (("person.json", "saved", os.path.join(s, "saved", "person.json"), s, on), False),
        (("person.json", "person-scratch-0a1b2c3d", os.path.join(s, "person-scratch-0a1b2c3d", "..", "person.json"), s, on), False),
        (("person.json", "person-scratch-zzzzzzzz", os.path.join(s, "person-scratch-zzzzzzzz", "person.json"), s, on), False),
        (("person.json", "person-scratch-0a1b2c3d", "/elsewhere/person-scratch-0a1b2c3d/person.json", s, on), False),
    ]
    bad = [(c, want) for c, want in cases if may_press(*c)[0] is not want]
    for c, want in bad:
        print(f"PERSON FAIL | may_press{c[:3]} | wanted {want}, got {may_press(*c)}")
    print(f"PERSON {'PASS' if not bad else 'FAIL'} | the refusal before any Save press | {len(cases) - len(bad)} of {len(cases)} cases")
    return not bad


def check_sheet(name, opener, shot):
    r = step(f"menu-pick:{opener}")
    if not r.get("ok"):
        return say(False, name, f"the menu pick failed: {r.get('error')} (item {r.get('item')})")
    s = wait(lambda: (lambda d: d if d.get("ok") else None)(step("sheet")), 15, every=0.3)
    if not s:
        st = status()
        return say(False, name, f"no sheet appeared on the window: pick {r}; the app's last sheet {st.get('sheet')}, banner {st.get('banner')!r}; documents {docs()}")
    pic = step(f"sheet:{os.path.join(A.shots, shot)}").get("picture")
    axs = ax("sheet")
    problems, seen_ax = layout_ok(s["sheet"], axs.get("sheets") if axs.get("trusted") else None)
    ctl = "; ".join(f"{c['kind']} {c.get('axLabel') or c['text']!r}" for c in s["sheet"]["controls"] if c["kind"] != "label")
    ok = not problems
    say(ok, name, f"a sheet ({s['sheet']['class']}, {s['sheet']['size']}): {ctl}; picture {pic}; {seen_ax}" + ("" if ok else "; " + "; ".join(problems)))
    c = step("sheet-press:Cancel")
    gone = wait(lambda: not step("sheet").get("ok"), 10, every=0.3)
    say(bool(c.get("ok") and gone), f"{name}: Cancel closes it", "closed" if gone else f"still open ({c.get('error')})")
    return s["sheet"]




# The engine half (decision 10 after the person check: "add a launched test engine so the apps
# cover them"): the engine-interface floor rows driven through the launched app, on the installed
# engine (the text engine's program as a separate process, from the scratch engines folder), and
# asserted from what the app shows: the scene its canvas draws (read over the port), its engine
# cell and banner (the status file). A graph of the text engine's own testing nodes (text.slow
# reports progress and stops on cancel; text.fail always fails), each pinned to the installed
# engine, is opened as the Finder opens a file; Run, Cancel Run and Auto-Run are picked in the
# real Run menu; values are typed into the faces' fields. Each step names the floor rows it meets.
ENGINE_GRAPH = {
    "version": 1,
    "engines": ["person_text"],
    "nodes": {
        "story": {"kind": "text.literal", "pos": [0, 0], "engine": "person_text", "values": {"text": "a corgi naps"}},
        "think": {"kind": "text.slow", "pos": [320, 0], "engine": "person_text", "values": {"seconds": 3.0}},
        "rep": {"kind": "text.repeat", "pos": [640, 0], "engine": "person_text", "values": {"times": 2}},
        "shout": {"kind": "text.upper", "pos": [960, 0], "engine": "person_text"},
        "broken": {"kind": "text.fail", "pos": [320, 300], "engine": "person_text"},
        "after": {"kind": "text.lower", "pos": [640, 300], "engine": "person_text"},
    },
    "wires": [
        {"from": {"node": "story", "port": "text"}, "to": {"node": "think", "port": "text"}},
        {"from": {"node": "think", "port": "text"}, "to": {"node": "rep", "port": "text"}},
        {"from": {"node": "rep", "port": "text"}, "to": {"node": "shout", "port": "text"}},
        {"from": {"node": "story", "port": "text"}, "to": {"node": "broken", "port": "text"}},
        {"from": {"node": "broken", "port": "text"}, "to": {"node": "after", "port": "text"}},
    ],
}


def engine_half(port, outside):
    path = os.path.join(A.scratch, "engine-check.json")
    with open(path, "w") as f:
        json.dump(ENGINE_GRAPH, f)
    r = step(f"open:{path}")
    mine = wait(lambda: [d for d in docs() if d["path"] and os.path.realpath(d["path"]) == os.path.realpath(path) and d["ready"]], 30, every=0.3)
    if not (r.get("ok") and mine):
        say(False, "the engine half: the test graph opened", f"open {r}; documents {docs()}")
        return
    doc = mine[0]["doc"]
    k = next(i for i, d in enumerate(docs()) if d["doc"] == doc)
    step(f"use:{k}")

    def faces():
        try:
            return {n["id"]: n for n in port.call("scene.get", {}, doc=doc).get("nodes", [])}
        except RuntimeError:
            return {}

    def state(fs, n):
        return (fs.get(n, {}).get("run") or {}).get("state")

    def inspect_run():
        return port.call("ui.inspect", {}, doc=doc).get("run", {})

    def engine_up():
        e = port.call("ui.inspect", {}, doc=doc).get("engine", {})
        return e if e.get("connected") and any(x.get("id") == "person_text" and (x.get("state") or {}).get("state") == "connected" for x in e.get("engines", [])) else None

    up = wait(engine_up, 60, every=0.5)
    say(bool(up), "the engine half: the test graph's engine, the installed one, connected in its window",
        f"engines {[(x.get('id'), (x.get('state') or {}).get('state')) for x in (up or {}).get('engines', [])]}")

    def watch_run(secs, stop=None):
        """Every state each face shows while the run goes, sampled every 50 ms; ends when the run
        has ended (ui.inspect's run.running false after it started), or at secs."""
        seen = {}
        frames = {}
        t0 = time.time()
        started = False
        gen = None
        while time.time() - t0 < secs:
            fs = faces()
            for n, f in fs.items():
                st = state(fs, n)
                if st and (not seen.get(n) or seen[n][-1] != st):
                    seen.setdefault(n, []).append(st)
                fr = (f.get("run") or {}).get("fraction")
                if fr is not None:
                    frames.setdefault(n, []).append(fr)
            ir = inspect_run()
            running = ir.get("running")
            # a run that ended before the first look: its generation moved on (or its faces done)
            gen = ir.get("generation") if gen is None else gen
            started = started or bool(running) or (time.time() - t0 > 1.0 and ir.get("generation") != gen)
            ended = {"done", "cached", "error", "skipped", "cancelled"}
            if not running and time.time() - t0 > 1.5 and fs and all(state(fs, n) in ended for n in fs if state(fs, n)):
                started = True
            if stop and stop(fs):
                break
            if started and not running:
                break
            time.sleep(0.05)
        # the scene a moment after the run ends (it follows the run's last records): until no
        # face is still queued or working, at most 5 s
        busy = {"queued", "started", "running", "progress"}
        last = wait(lambda: (lambda f: f if f and not any(state(f, n) in busy for n in f) else None)(faces()), 5, every=0.1) or faces()
        for n in last:
            st = state(last, n)
            if st and (not seen.get(n) or seen[n][-1] != st):
                seen.setdefault(n, []).append(st)
        return seen, frames, last

    # E1. Run from the Run menu: queued faces before the engine says anything (F31), states
    # streaming as the engine reports them (F38, F39), progress (F38), an error as data on its
    # face (F34), the node after it skipped (F39), the text on the face (F46), the time each took
    # (F42), the run's identity (F28)
    gen0 = inspect_run().get("generation", 0)
    r = step("menu-pick:Run/Run")
    seen, frames, fs = watch_run(30)
    run1 = inspect_run()
    say(bool(r.get("ok")) and "queued" in seen.get("shout", []) and seen.get("shout", [""])[-1] == "done",
        "F31 optimistic queued: the faces show queued as Run is picked, before the engine reports them",
        f"pick {r.get('ok')} {r.get('error') or ''}; states shout {seen.get('shout')}, rep {seen.get('rep')}")
    prog = [x for x in frames.get("think", []) if 0 < x < 1]
    say(("progress" in seen.get("think", [])) and len(set(prog)) >= 2,
        "F38 status stream, F39 progress: the slow node's face shows its progress as the engine streams it",
        f"think's states {seen.get('think')}; fractions seen {sorted(set(round(x, 2) for x in prog))[:12]}")
    br = fs.get("broken", {})
    err = next((x for x in br.get("footer", []) if x.get("kind") == "error"), None)
    say(state(fs, "broken") == "error" and bool(err) and "always fails" in (err or {}).get("full", "") + (err or {}).get("text", ""),
        "F34 errors as data, F39 error: the failing node's face shows its error from the engine",
        f"broken {seen.get('broken')}; its footer {err}")
    say(state(fs, "after") == "skipped", "F39 skipped: the node after the failing one is skipped, not run",
        f"after {seen.get('after')}")
    ex = next((x for x in fs.get("shout", {}).get("footer", []) if x.get("kind") == "excerpt"), None)
    say(bool(ex) and "A CORGI NAPS" in ex.get("text", ""), "F46 text on nodes: the result's text is on its face",
        f"shout's excerpt {ex}")
    hs = [s.get("text", "") for s in fs.get("think", {}).get("header", {}).get("status", [])]
    timed = [t for t in hs if any(c.isdigit() for c in t) and ("s" in t)]
    say(bool(timed), "F42 per-node timing: the slow node's face says how long it took", f"think's status {hs}")
    gen1 = run1.get("generation", 0)
    say(gen1 > gen0 and run1.get("runId") is not None, "F28 run identity: the run has an id and the generation rose",
        f"generation {gen0} -> {gen1}, runId {run1.get('runId')}")

    # E2. F45: an edit upstream makes what it feeds stale on the face before anything runs
    step("edit:story.text")
    step("field:a corgi sleeps")
    step("return")
    stale = wait(lambda: (lambda f: f if (f.get("shout", {}).get("run") or {}).get("stale") else None)(faces()), 10, every=0.2)
    say(bool(stale) and not inspect_run().get("running"), "F45 stale tracking: an edit upstream marks what it feeds stale, nothing run",
        f"shout's run {(faces().get('shout') or {}).get('run')}")

    # E3. F36 Cancel: Run, then Run › Cancel Run while the slow node works: it stops (cancelled)
    # and nothing after it runs
    step("menu-pick:Run/Run")
    wait(lambda: state(faces(), "think") == "progress", 10, every=0.05)
    t0 = time.time()
    c = step("menu-pick:Run/Cancel Run")
    seen, _, fs = watch_run(10)
    took = time.time() - t0
    say(bool(c.get("ok")) and state(fs, "think") == "cancelled" and state(fs, "shout") != "done" and took < 3,
        "F36 cancel: Run › Cancel Run stops the slow node at once; nothing after it runs",
        f"pick {c.get('ok')} {c.get('error') or ''}; think {seen.get('think')}, shout {seen.get('shout')}; ended {took:.1f} s after the pick")

    # E4. F29 latest wins: Run, then an edit and Run again before it ends: the first is
    # superseded, the result is the last edit's
    sup0 = inspect_run().get("superseded", 0)
    step("menu-pick:Run/Run")
    wait(lambda: state(faces(), "think") == "progress", 10, every=0.05)
    step("edit:story.text")
    step("field:a corgi wakes")
    step("return")
    step("menu-pick:Run/Run")
    seen, _, fs = watch_run(30)
    ex = next((x for x in fs.get("shout", {}).get("footer", []) if x.get("kind") == "excerpt"), {})
    sup1 = inspect_run().get("superseded", 0)
    say(sup1 > sup0 and "A CORGI WAKES" in ex.get("text", "") and state(fs, "shout") == "done",
        "F29 latest wins: a run started over one under way supersedes it; the faces show the last edit's result",
        f"superseded {sup0} -> {sup1}; shout {seen.get('shout')}, its text {ex.get('text')!r}")

    # E5. F72 partial re-run: a value changed after the slow node: only it and what it feeds run;
    # the slow node is cached
    step("edit:rep.times")
    step("field:3")
    step("return")
    step("menu-pick:Run/Run")
    seen, _, fs = watch_run(20)
    say(state(fs, "think") == "cached" and state(fs, "rep") == "done" and state(fs, "story") == "cached",
        "F72 partial re-run: after a change downstream, only what it touches runs; the slow node is cached",
        f"story {seen.get('story')}, think {seen.get('think')}, rep {seen.get('rep')}, shout {seen.get('shout')}")

    # E6. F30 Auto-run: Run › Auto-Run on, an edit runs by itself; then off
    a = step("menu-pick:Run/Auto-Run")
    checked = [i.get("checked") for i in step("menu:Run").get("items", []) if i.get("title") == "Auto-Run"]
    step("edit:rep.times")
    step("field:4")
    step("return")
    auto = wait(lambda: (lambda f: f if state(f, "rep") == "done" and "a corgi wakes" * 4 in json.dumps(f.get("rep", {})) or (state(f, "shout") == "done" and "A CORGI WAKES" * 4 in json.dumps(f.get("shout", {}))) else None)(faces()), 20, every=0.2)
    step("menu-pick:Run/Auto-Run")
    off = [i.get("checked") for i in step("menu:Run").get("items", []) if i.get("title") == "Auto-Run"]
    say(bool(a.get("ok") and checked == [True] and auto and off == [False]),
        "F30 auto-run: with Run › Auto-Run checked an edit runs by itself; unchecked again",
        f"pick {a.get('ok')}; checked {checked} then {off}; ran by itself {bool(auto)}")

    # E7. F35 a failure stands in for a reply: the engine's process killed in the middle of the
    # slow node's work: the run ends (it doesn't wait for a reply that can't come), the face says
    # so, and the engine comes back
    step("edit:story.text")
    step("field:a corgi dreams")
    step("return")
    step("menu-pick:Run/Run")
    wait(lambda: state(faces(), "think") == "progress", 10, every=0.05)
    pids, _ = outside()
    for p in pids:
        try:
            os.kill(int(p), 9)
        except OSError:
            pass
    t0 = time.time()
    seen, _, fs = watch_run(30)
    took = time.time() - t0
    back = wait(engine_up, 30, every=0.5)
    say(bool(pids) and state(fs, "think") in ("error", "cancelled") and took < 15 and bool(back),
        "F35 a failure stands in for a reply: the engine killed mid-node ends the run with the face saying so; the engine starts again",
        f"killed {pids}; think {seen.get('think')} ({next((x.get('text') for x in fs.get('think', {}).get('footer', []) if x.get('kind') == 'error'), None)!r}); "
        f"the run ended {took:.1f} s after; engine back {bool(back)}; banner {status().get('banner')!r}")

    # the window goes as it came: saved (to scratch) so it closes without asking, then closed
    port.call("command.run", {"command": "graph.saveAs", "params": {"path": os.path.join(A.scratch, "engine-check-saved.json")}}, doc=doc)
    wait(lambda: all(not d["edited"] for d in docs() if d["doc"] == doc), 10, every=0.3)
    step("menu-pick:File/Close")
    wait(lambda: all(d["doc"] != doc for d in docs()), 15, every=0.3)
    step("use:0")


def main():
    global A
    p = argparse.ArgumentParser()
    for a in ("pid", "status", "inbox", "token", "ax", "scratch", "shots", "engine-label", "engine-bin"):
        p.add_argument("--" + a, required=True)
    p.add_argument("--self-test", action="store_true")
    A = p.parse_args()
    A.pid = int(A.pid)
    os.makedirs(A.shots, exist_ok=True)

    # 1. launched with nothing: one untitled graph window, ready (B70)
    st = wait(lambda: (lambda s: s if s.get("port") and s.get("windows") and s["windows"][0].get("ready") else None)(status()), 60)
    if not st:
        say(False, "launch: an untitled graph window", f"none within 60 s: {status()}")
        return
    ds = docs()
    axw = graph_windows_ax()
    say(len(ds) == 1 and ds[0]["path"] == "" and ds[0]["visible"],
        "launch: an untitled graph window", f"documents {[(d['title'], d['path'] or 'no file', 'visible' if d['visible'] else 'hidden') for d in ds]}"
        + (f"; accessibility: windows {[w.get('title') for w in axw]}" if axw is not None else "; accessibility not trusted"))
    port = Port()

    # 1b. B87: the engines folder's unreadable manifest is said: the engine's state and the
    # window's banner (the core's status line, which the Mac shows as its banner)
    eng = port.call("ui.inspect", {}).get("engine", {})
    probs = eng.get("manifestProblems") or []
    banner = wait(lambda: (lambda b: b if b and "broken.toml" in b else None)(status().get("banner")), 15, every=0.3)
    say(any("broken.toml" in p for p in probs) and bool(banner), "an unreadable manifest is said at launch (B87)",
        f"manifestProblems {probs}; the window's banner {status().get('banner')!r}")

    # 2. File › New makes another (B70)
    r = step("menu-pick:File/New Graph")
    two = wait(lambda: (lambda d: d if len(d) == 2 and all(x["ready"] for x in d) else None)(docs()), 30, every=0.3)
    axw = graph_windows_ax()
    say(bool(r.get("ok") and two), "File › New: another untitled graph, made by the core",
        f"pick {r.get('ok')} ({r.get('target') or r.get('error')}); the core-made window {status().get('documentWindows')}; documents {[d['title'] for d in (two or docs())]}"
        + (f"; accessibility: windows {[w.get('title') for w in axw]}" if axw is not None else ""))
    step("use:0")

    # 3. the Engine menu: the built-in engines and one installed in the (scratch) engines folder
    items = step("menu:Engine").get("items", [])
    sub = [("Engine/" + i["title"], i) for i in items if "title" in i]
    for i in items:
        if i.get("submenu") and i.get("title"):
            sub += [(f"Engine/{i['title']}/{x['title']}", x) for x in step(f"menu:Engine/{i['title']}").get("items", []) if "title" in x]
    alltitles = [p for p, _ in sub]
    blank = [p for p, x in sub if not x.get("title", "").strip()]
    has_text = any(p.startswith("Engine/Text") for p in alltitles)
    has_mine = f"Engine/{A.engine_label}" in alltitles
    say(has_text and has_mine and not blank, "Engine menu: built-in engines and the installed one, by name",
        f"items {[p[len('Engine/'):] for p in alltitles]}" + (f"; untitled items {len(blank)}" if blank else ""))
    mine_path = f"Engine/{A.engine_label}"

    # 4. the installed engine, picked in the Engine menu: started as a process of the app's own
    r = step(f"menu-pick:{mine_path}")
    eng = wait(lambda: (lambda e: e if e and e.get("connected") and "2 engines" in (e.get("label") or "") else None)(status().get("engine")), 60, every=0.3)

    def outside():
        kids = subprocess.run(["pgrep", "-P", str(A.pid)], capture_output=True, text=True).stdout.split()
        comms = {c: subprocess.run(["ps", "-o", "comm=", "-p", c], capture_output=True, text=True).stdout.strip() for c in kids}
        return [c for c, comm in comms.items() if os.path.realpath(comm) == os.path.realpath(A.engine_bin)], comms
    mine, comms = outside()
    checked = [i.get("title") for i in step("menu:Engine").get("items", []) if i.get("checked")]
    e = status().get("engine", {})
    say(bool(r.get("ok") and eng and mine), "Engine › the installed engine: started as a separate process",
        f"pick {r.get('ok')} ({r.get('error') or 'sent'}); engine cell {e.get('shown')!r}, {e.get('label')!r}; checked {checked}; the app's child processes {comms}")

    # 5. two nodes from the palette, run on the installed engine (each node's Run on menu), a value
    # typed on the face, wired by a drag, run, the result popped out by a click on its pin
    doc = ds[0]["doc"]

    def nodes():
        return {n["id"]: n for n in port.call("ui.inspect", {}, doc=doc).get("graph", {}).get("nodes") or []}

    def win():
        return next((x for x in status().get("windows", []) if x.get("doc") == doc), {})
    before = set(nodes())
    for kind, at, want in (("text.literal", "at=80,80", 1), ("text.upper", "at=420,80", 2)):
        step(f"palette:{at}")
        step("type:" + kind.split(".")[1])
        step(f"pick:{kind}")
        wait(lambda: len(set(nodes()) - before) >= want and win().get("faces", 0) >= want, 20, every=0.3)
    g = nodes()
    new = [n for n in g if n not in before]
    lit = next((n for n in new if g[n].get("kind") == "text.literal"), None)
    up = next((n for n in new if g[n].get("kind") == "text.upper"), None)
    say(bool(lit and up), "the palette adds a Text and an Upper case node", f"new nodes {[(n, g[n].get('kind'), g[n].get('pos')) for n in new]}")
    if lit and up:
        ran_on = [step(f"context-pick:{n}:Run on/{A.engine_label}") for n in (lit, up)]
        def engines_shown():
            scene = port.call("scene.get", {}, doc=doc)
            return {n["id"]: n["header"]["engine"]["text"] for n in scene.get("nodes", []) if n["id"] in (lit, up)}
        hdr = wait(lambda: (lambda h: h if len(h) == 2 and all("person_text" in t for t in h.values()) else None)(engines_shown()), 10, every=0.3) or engines_shown()
        say(all(x.get("ok") for x in ran_on) and len(hdr) == 2 and all("person_text" in t for t in hdr.values()),
            f"each node's context menu › Run on › {A.engine_label} (by its name, B86)", f"picks {[x.get('ok') or x.get('error') for x in ran_on]}; faces say {hdr}")
        step(f"edit:{lit}.text")
        step("field:hello person")
        step("return")
        set_ok = wait(lambda: win().get("undo") == "Set Value", 10, every=0.3)
        say(bool(set_ok), "a value typed into the Text node's field", f"undo {win().get('undo')!r}")
        step(f"press:out={lit}.0")
        step(f"moveto:in={up}.0")
        step("release")
        wired = wait(lambda: win().get("wireCount", 0) >= 1, 10, every=0.3)
        say(bool(wired), "a wire dragged from Text to Upper case", f"wires {win().get('wireCount')}, undo {win().get('undo')!r}")
        step("run")
        port.call("ui.waitRun", {"timeout": 60}, doc=doc)
        step(f"tap:out={up}.0")
        pops = wait(lambda: win().get("popouts"), 20, every=0.3)
        val = json.dumps(port.call("ui.inspect", {}, doc=doc).get("watched"))[:300]
        mine2, _ = outside()
        say(bool(pops) and "HELLO PERSON" in val and bool(mine2), "Run, and the value popped out",
            f"pop-outs {json.dumps(pops)[:300]}; watched {val}; the installed engine's process {mine2}")
        # F2: the installed engine's process dies (killed by its PID, a child of this check's app):
        # the engine cell says so, the core starts it again (its manifest's restart = on-crash),
        # and the next run computes on the new process
        if mine2:
            old = mine2[0]
            os.kill(int(old), 9)
            down = wait(lambda: (lambda e: e if e and (not e.get("connected") or "down" in (e.get("shown") or "").lower()) else None)(status().get("engine")), 10, every=0.1)
            again = wait(lambda: (lambda m: m if m and m[0] != old else None)(outside()[0]), 30, every=0.3)
            up = wait(lambda: (lambda e: e if e and e.get("connected") and "2 engines" in (e.get("label") or "") else None)(status().get("engine")), 30, every=0.3)
            step(f"edit:{lit}.text")
            step("field:once more")
            step("return")
            step("run")
            port.call("ui.waitRun", {"timeout": 60}, doc=doc)
            val2 = json.dumps(port.call("ui.inspect", {}, doc=doc).get("watched"))[:300]
            say(bool(again and up and "ONCE MORE" in val2), "the installed engine's process killed: started again, runs again (F2)",
                f"killed {old}; engine cell meanwhile {down}; new process {again}; engine {status().get('engine', {}).get('label')!r}; watched {val2}")
        # the pop-out's bar: Recipe (decision 27), pressed as an assistive app presses it
        title = (pops or [{}])[0].get("title")
        pr = subprocess.run([A.ax, "press", str(A.pid), "viewer-recipe", title or ""], capture_output=True, text=True, timeout=30).stdout.strip()
        rec = wait(lambda: status().get("recipe"), 10, every=0.3)
        say(bool(rec and rec.get("shown")), "the pop-out's Recipe", f"pressed {pr}; the app says {rec}")

        # the engine half (decision 10): the engine-interface rows on the installed engine
        engine_half(port, outside)

    # 6, 7. Attach… and Install Engine… show real sheets, labels beside their fields (B73)
    check_sheet("Engine › Attach…: a real sheet", "Engine/Attach…", "attach-sheet.png")
    inst = check_sheet("Engine › Install Engine…: a real sheet", "Engine/Install Engine…", "install-sheet.png")
    combo = [c for c in (inst or {}).get("controls", []) if c["kind"] in ("combo", "popup") and c.get("axLabel") == "Repository"]
    say(bool(combo) and len(combo[0].get("choices", [])) >= 3, "Install Engine…: the repository is a pick list with any other typed (decision 5)",
        f"{combo[0]['kind']} with {combo[0].get('choices')}" if combo else f"no pick list: {[(c['kind'], c.get('axLabel')) for c in (inst or {}).get('controls', [])]}")

    # 8. File › Save As…: the real save panel, set before it shows to a scratch folder of a name
    # no folder of the person's has, read through accessibility (the panel's own name field and
    # folder, from outside: the panel runs out of process), asserted, then cancelled. Save is never
    # pressed (the maintainer, 2026-11-21: "1,2 as you say"; a save once landed in ~/Documents):
    # may_press refuses unless PERSON_PRESS_SAVE=1, which no script sets, and both fields are the
    # scratch ones. The file is written by the core's graph.saveAs, to that scratch folder.
    folder = os.path.join(A.scratch, f"person-scratch-{os.urandom(4).hex()}")
    os.makedirs(folder, exist_ok=True)
    path = os.path.join(folder, "person.json")

    def opened(p):
        return [x for x in docs() if x["path"] and os.path.realpath(x["path"]) == os.path.realpath(p) and x["ready"]]

    def faces_of(d):
        return next((w.get("faces") for w in status().get("windows", []) if w.get("doc") == d), None)

    at = step(f"panel-at:{path}")
    r = step("menu-pick:File/Save As…")
    panel = wait(lambda: (lambda d: d if d.get("ok") and "Panel" in d["sheet"]["class"] else None)(step("sheet")), 15, every=0.3)
    seen = {}
    if panel:
        seen = wait(lambda: (lambda f: f if f.get("name") is not None and f.get("folder") is not None else None)(panel_fields(sheet=True)), 10, every=0.5) or panel_fields(sheet=True)
    allowed, why = may_press(seen.get("name"), seen.get("folder"), path, A.scratch, os.environ)
    if allowed:
        # never reached by any script (PERSON_PRESS_SAVE is set by none)
        axrun("press", str(A.pid), "OKButton")
    else:
        axrun("press", str(A.pid), "CancelButton")
    gone = wait(lambda: not step("sheet").get("ok"), 10, every=0.3)
    say(bool(at.get("ok") and panel and seen.get("name") == "person.json" and seen.get("folder") == os.path.basename(folder) and gone and not allowed),
        "File › Save As…: the save panel opens on the scratch folder with the name, read from the panel itself, then Cancel",
        f"pick {r.get('ok')}; panel {panel['sheet']['class'] if panel else None}; the panel's own name field {seen.get('name')!r}, "
        f"folder {seen.get('folder')!r} (wanted {os.path.basename(folder)!r}); Save not pressed: {why}; cancelled {bool(gone)}")
    port.call("command.run", {"command": "graph.saveAs", "params": {"path": path}}, doc=doc)
    on_disk = wait(lambda: os.path.exists(path) and "nodes" in open(path).read(), 10, every=0.3)
    clean = wait(lambda: (lambda d: d if d and not d[0]["edited"] and os.path.realpath(d[0]["path"] or "/nonexistent") == os.path.realpath(path) else None)(docs()), 10, every=0.3)
    # the app noted the file as a recent document (AppKit asks the count from a queue of its own:
    # the scripted copy's first such note stopped the app before the count was nonisolated)
    time.sleep(1.5)
    say(bool(on_disk and clean and alive()), "the graph saved (graph.saveAs, to scratch), the edited dot cleared, the app still running after noting it as recent",
        f"on disk {bool(on_disk)}; document {(docs() or [None])[0]}; alive {alive()}")

    # 9. close it
    r = step("menu-pick:File/Close")
    closed = wait(lambda: (lambda d: d if len(d) == 1 else None)(docs()), 15, every=0.3)
    say(bool(r.get("ok") and closed), "File › Close", f"documents {[d['title'] for d in docs()]}")

    # 10. File › Open Recent: the saved graph is listed (the scripted copy's own list, B81), picked
    name = os.path.basename(path)

    def recent():
        return [i.get("title") for i in step("menu:File/Open Recent").get("items", []) if i.get("title")]
    # AppKit fills the menu from the list it reads in the background: asked again until it shows
    titles = wait(lambda: (lambda t: t if name in t else None)(recent()), 10, every=0.5) or recent()
    listed = step("menu:File/Open Recent").get("recent") or []
    mine = [p for p in listed if os.path.realpath(p) == os.path.realpath(path)]
    if name in titles:
        rp = step(f"menu-pick:File/Open Recent/{name}")
    elif mine:
        # AppKit fills Open Recent's items only while the menu tracks (a person's click), which
        # this check may not start (it never takes the pointer): the item it would show is the
        # document controller's entry, opened as the item opens it
        say(None, "File › Open Recent's items", f"the menu outside tracking shows {titles}; its list (the document controller's) {listed}")
        rp = step(f"open:{mine[0]}")
    else:
        rp = {"ok": False, "error": f"not listed: menu {titles}, list {listed}"}
    back = wait(lambda: opened(path), 20, every=0.3)
    n = (wait(lambda: faces_of(back[0]["doc"]) == 2 and 2, 30, every=0.5) or faces_of(back[0]["doc"])) if back else None
    say(bool(mine and rp.get("ok") and back and n == 2), "File › Open Recent: the saved graph in the scripted copy's own recent list, opened from it, and shown",
        f"items {titles}; the document controller's list {listed}; open {rp.get('ok')} {rp.get('error') or ''}; documents {[(d['title'], d['path'], d['ready']) for d in docs()]}; faces {n}")

    # 10b. File › Open… (the core's graph.open, decision 20): closed again; the real open panel
    # shown at the scratch folder, its folder read from the panel itself, cancelled; then the
    # core's graph.open answered by the script opens the file
    if back:
        kk = next((i for i, d in enumerate(docs()) if d["doc"] == back[0]["doc"]), 0)
        step(f"use:{kk}")
        step("menu-pick:File/Close")
        wait(lambda: all(d["doc"] != back[0]["doc"] for d in docs()), 15, every=0.3)
    step("use:0")
    step(f"panel-at:{path}")
    r = step("menu-pick:File/Open…")
    last = {}

    def open_panel():
        last.clear()
        last.update(panel_fields(sheet=False))
        return last if last.get("folders") else None
    wait(open_panel, 15, every=0.5)
    oseen = dict(last)
    axrun("press", str(A.pid), "CancelButton")
    closed_panel = wait(lambda: not panel_fields(sheet=False).get("folders"), 10, every=0.5)
    say(bool(r.get("ok") and os.path.basename(folder) in (oseen.get("folders") or []) and closed_panel),
        "File › Open…: the open panel opens on the scratch folder, read from the panel itself, then Cancel",
        f"pick {r.get('ok')} ({r.get('target') or r.get('error')}); the panel's own folder pop-up(s) {oseen.get('folders')}; Open not pressed; cancelled {bool(closed_panel)}"
        + ("" if oseen.get("folders") else f"; windows seen {oseen.get('windows')}"))
    inv = port.call("ui.invoke", {"command": "graph.open"})
    ask = next((a for a in inv.get("asks", []) if a.get("kind") == "file.open"), None)
    if ask:
        port.call("ask.answer", {"ask": ask["ask"], "answer": {"paths": [path]}})
    got = wait(lambda: opened(path), 20, every=0.3)
    faces = wait(lambda: (lambda f: f if f == 2 else None)(faces_of(got[0]["doc"])), 30, every=0.5) if got else None
    say(bool(ask and got and faces == 2), "File › Open… through the core: the window shows the file",
        f"ask {ask and ask.get('kind')} ({ask and ask.get('params', {}).get('purpose')}); documents {[(d['title'], d['path']) for d in docs()]}; "
        f"the app's last core-made window {status().get('documentWindows')}; faces {faces}")

    # 10c. decision 17: the copied manifest's engine is marked in the Engine menu (the warning
    # symbol as the item's image, the warning as its tooltip) and, picked so it is in use, in the
    # engine cell (its stand-in uv starts the text engine's program, so it does start)
    step("use:0")
    items = step("menu:Engine").get("items", [])
    cp = next((i for i in items if i.get("title") == "Copied Engine"), {})
    plain = next((i for i in items if (i.get("title") or "").startswith("Text")), {})
    say(bool(cp.get("image") and "may be a copy" in (cp.get("image") or "") and "may be a copy" in (cp.get("tooltip") or "") and cp.get("enabled") and not plain.get("image")),
        "the Engine menu marks the copied manifest's engine (decision 17)",
        f"Copied Engine: image {cp.get('image')!r}, tooltip {(cp.get('tooltip') or '')[:120]!r}, enabled {cp.get('enabled')}; Text's image {plain.get('image')!r}")
    r = step("menu-pick:Engine/Copied Engine")
    cell = wait(lambda: (lambda e: e if e and "Copied Engine" in (e.get("warning") or "") else None)(status().get("engine")), 20, every=0.3) or status().get("engine", {})
    say(bool(r.get("ok") and cell.get("warning") and "may be a copy" in (cell.get("tooltip") or "")),
        "the engine cell marks the copied engine once it is in use (decision 17)",
        f"pick {r.get('ok')} {r.get('error') or ''}; engine cell {cell.get('shown')!r}, warning {cell.get('warning')!r}, tooltip {(cell.get('tooltip') or '')[:160]!r}")

    # 11. every window closed, then a click on the Dock icon: a new untitled graph (B70)
    for _ in range(len(docs())):
        step("use:0")
        step("menu-pick:File/Close")
    none = wait(lambda: not docs(), 15, every=0.3)
    r = step("dock-reopen")
    fresh = wait(lambda: (lambda d: d if len(d) == 1 and d[0]["path"] == "" and d[0]["ready"] else None)(docs()), 20, every=0.3)
    say(bool(none and fresh), "every window closed, then the Dock icon: a new untitled graph",
        f"after closing {[d['title'] for d in docs()] if not fresh else []}; reopen {r.get('ok')} (had visible {r.get('hadVisible')}); documents {[d['title'] for d in (fresh or docs())]}")
    step("use:0")

    # 12. quit from the menu, as a person does (the shell script waits for the PID to go)
    r = step("menu-pick:Plummer/Quit Plummer", secs=5)
    print(f"PERSON QUIT | picked Quit Plummer: {r.get('ok', 'no reply (quitting)')}", flush=True)


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        sys.exit(0 if self_test() else 1)
    main()
    sys.exit(1 if any(r is False for r in results) or not results else 0)
