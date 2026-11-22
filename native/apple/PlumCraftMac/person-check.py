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


def main():
    global A
    p = argparse.ArgumentParser()
    for a in ("pid", "status", "inbox", "token", "ax", "scratch", "shots", "engine-label", "engine-bin"):
        p.add_argument("--" + a, required=True)
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
        ran_on = [step(f"context-pick:{n}:Run on/person_text") for n in (lit, up)]
        def engines_shown():
            scene = port.call("scene.get", {}, doc=doc)
            return {n["id"]: n["header"]["engine"]["text"] for n in scene.get("nodes", []) if n["id"] in (lit, up)}
        hdr = wait(lambda: (lambda h: h if len(h) == 2 and all("person_text" in t for t in h.values()) else None)(engines_shown()), 10, every=0.3) or engines_shown()
        say(all(x.get("ok") for x in ran_on) and len(hdr) == 2 and all("person_text" in t for t in hdr.values()),
            "each node's context menu › Run on › person_text", f"picks {[x.get('ok') or x.get('error') for x in ran_on]}; faces say {hdr}")
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

    # 6, 7. Attach… and Install Engine… show real sheets, labels beside their fields (B73)
    check_sheet("Engine › Attach…: a real sheet", "Engine/Attach…", "attach-sheet.png")
    inst = check_sheet("Engine › Install Engine…: a real sheet", "Engine/Install Engine…", "install-sheet.png")
    combo = [c for c in (inst or {}).get("controls", []) if c["kind"] in ("combo", "popup") and c.get("axLabel") == "Repository"]
    say(bool(combo) and len(combo[0].get("choices", [])) >= 3, "Install Engine…: the repository is a pick list with any other typed (decision 5)",
        f"{combo[0]['kind']} with {combo[0].get('choices')}" if combo else f"no pick list: {[(c['kind'], c.get('axLabel')) for c in (inst or {}).get('controls', [])]}")

    # 8. Save As: the panel is seen, then cancelled. AppKit's save panel keeps the folder it shows
    # in the app's defaults domain (NSOSPLastRootDirectory), which can't be pointed elsewhere
    # (B50), so moving it to a scratch folder changes the person's Library, and saving where it
    # opens writes into a folder of the person's. The file is written by the core's graph.saveAs.
    path = os.path.join(A.scratch, "person.json")

    def axrun(*a):
        try:
            return json.loads(subprocess.run([A.ax, *a], capture_output=True, text=True, timeout=30).stdout)
        except (subprocess.SubprocessError, ValueError) as e:
            return {"error": str(e)}
    r = step("menu-pick:File/Save As…")
    panel = wait(lambda: (lambda d: d if d.get("ok") and "Panel" in d["sheet"]["class"] else None)(step("sheet")), 15, every=0.3)
    seen = []
    if panel:
        time.sleep(0.5)
        els = [e for s in axrun("sheet", str(A.pid)).get("sheets", []) for e in s.get("elements", [])]
        seen = [e.get("identifier") for e in els if e.get("identifier") in ("saveAsNameTextField", "where popup", "OKButton", "CancelButton")]
        axrun("press", str(A.pid), "Cancel")
    gone = wait(lambda: not step("sheet").get("ok"), 10, every=0.3)
    say(bool(panel and len(seen) == 4 and gone), "File › Save As…: the save panel, then Cancel",
        f"pick {r.get('ok')}; panel {panel['sheet']['class'] if panel else None}; accessibility shows {seen}; cancelled {bool(gone)}")
    say(None, "File › Save As…: saving through the panel",
        "not done: the panel would write the person's defaults (its folder) or save into a folder of theirs; the core's graph.saveAs writes the file")
    port.call("command.run", {"command": "graph.saveAs", "params": {"path": path}}, doc=doc)
    on_disk = wait(lambda: os.path.exists(path) and "nodes" in open(path).read(), 10, every=0.3)
    clean = wait(lambda: (lambda d: d if d and not d[0]["edited"] else None)(docs()), 10, every=0.3)
    say(bool(on_disk and clean), "the graph saved (graph.saveAs) and the window's edited dot cleared", f"on disk {bool(on_disk)}; document {docs()[0] if docs() else None}")

    # 9. close it, 10. open it again
    r = step("menu-pick:File/Close")
    closed = wait(lambda: (lambda d: d if len(d) == 1 else None)(docs()), 15, every=0.3)
    say(bool(r.get("ok") and closed), "File › Close", f"documents {[d['title'] for d in docs()]}")
    # File › Open Recent is not opened: a scripted launch keeps no recent documents (B50), and
    # filling its menu on the first launch of a new build rewrote the person's list (B81)
    say(None, "File › Open Recent", "not seen: a scripted launch notes no recent document (B50); the check opens the file as Open Recent and the Finder do")
    r = step(f"open:{path}")
    back = wait(lambda: (lambda d: [x for x in d if x["path"] and os.path.realpath(x["path"]) == os.path.realpath(path) and x["ready"]])(docs()), 20, every=0.3)
    n = None
    if back:
        # what the window shows (the control channel answers for the first document only, B83)
        def count():
            return next((w.get("faces") for w in status().get("windows", []) if w.get("doc") == back[0]["doc"]), None)
        n = wait(lambda: count() == 2 and 2, 30, every=0.5) or count()
    say(bool(r.get("ok") and back and n == 2), "the saved graph opened again (as the Finder and Open Recent open it)", f"open {r.get('ok')} {r.get('error') or ''}; documents {[(d['title'], d['path'], d['ready']) for d in docs()]}; nodes {n}")

    # 10b. File › Open… is the core's (decision 20): closed again, then opened through the core's
    # graph.open, whose Open panel a script answers (the panel itself is not driven: AppKit keeps
    # its folder in the person's defaults); the core opens it into the untouched window or makes
    # a document whose window the feed's window domain brings
    if back:
        k = next((i for i, d in enumerate(docs()) if d["doc"] == back[0]["doc"]), 0)
        step(f"use:{k}")
        step("menu-pick:File/Close")
        wait(lambda: all(d["doc"] != back[0]["doc"] for d in docs()), 15, every=0.3)
        step("use:0")
        inv = port.call("ui.invoke", {"command": "graph.open"})
        ask = next((a for a in inv.get("asks", []) if a.get("kind") == "file.open"), None)
        if ask:
            port.call("ask.answer", {"ask": ask["ask"], "answer": {"paths": [path]}})
        got = wait(lambda: (lambda d: [x for x in d if x["path"] and os.path.realpath(x["path"]) == os.path.realpath(path) and x["ready"]])(docs()), 20, every=0.3)
        faces = None
        if got:
            faces = wait(lambda: (lambda f: f if f == 2 else None)(next((w.get("faces") for w in status().get("windows", []) if w.get("doc") == got[0]["doc"]), None)), 30, every=0.5)
        say(bool(ask and got and faces == 2), "File › Open… through the core: the window shows the file",
            f"ask {ask and ask.get('kind')} ({ask and ask.get('params', {}).get('purpose')}); documents {[(d['title'], d['path']) for d in docs()]}; "
            f"the app's last core-made window {status().get('documentWindows')}; faces {faces}")

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
    main()
    sys.exit(1 if any(r is False for r in results) or not results else 0)
