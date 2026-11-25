"""Bin an Instruments 'Animation Hitches' trace's hitches by the bench's phase signposts.

Usage: python3 s6-hitches.py PROCESS trace-signpost.xml trace-hitches.xml  (N3 slice 5: s3-hitches.py reading the fullest table that has its own schema; a device trace exports
several tables and the fullest had none)
(export both with `xctrace export --xpath '/trace-toc/run[@number="1"]/data/table[@schema="os-signpost"]'`, and "hitches").
"""
import re
import sys
import xml.etree.ElementTree as ET


def rows(path):
    """Rows of every table in an export (N3 budget stage: s5-hitches.py read only the fullest node
    that had a schema of its own; an export can hold several tables, and a table's rows can come in
    a node after its schema). An empty or broken export has none."""
    try:
        root = ET.parse(path).getroot()
    except (ET.ParseError, OSError) as e:
        print(f"(no table in {path}: {e})", file=sys.stderr)
        return
    ids = {}
    for el in root.iter():  # ids are shared across the whole document
        if "id" in el.attrib:
            ids.setdefault(el.attrib["id"], el)
    cols = None
    for node in root.findall("node"):
        # a table's schema can come in a node of its own, its rows in the next one with none (seen
        # on the Mac and on the iPad): the rows read with the schema before them. s5-hitches.py
        # skipped such rows, so a trace whose phases came that way showed no phases at all
        schema = node.find("schema")
        if schema is not None:
            cols = [c.findtext("mnemonic") for c in schema.findall("col")]
        if cols is None:
            continue
        for row in node.findall("row"):
            vals = {}
            for col, el in zip(cols, list(row)):
                if "ref" in el.attrib:
                    el = ids.get(el.attrib["ref"], el)
                vals[col] = (el.text, el.attrib.get("fmt"), el)
            yield vals


def main(proc_name, sp_path, hi_path):
    global PROC
    PROC = proc_name
    begins, phases = {}, []
    for r in rows(sp_path):
        name = (r.get("name") or (None, None))[1] or (r.get("name") or (None,))[0]
        if name != "phase":
            continue
        if "time" not in r:
            continue
        t = int(r["time"][0])
        msg = r.get("message", (None, None, None))
        text = msg[1] or msg[0] or ""
        if text == "" and msg[2] is not None:
            text = "".join(msg[2].itertext())
        sid = r.get("identifier", (None, None))[0]
        kind = (r.get("event-type") or (None, None))[1] or r.get("event-type", ("",))[0]
        if kind and kind.lower().startswith("begin"):
            begins[sid] = (t, text)
        elif kind and kind.lower().startswith("end") and sid in begins:
            t0, label = begins.pop(sid)
            phases.append((t0, t, label))
    hitches = []
    for r in rows(hi_path):
        proc = r.get("process", (None, ""))[1] or ""
        if PROC not in proc:
            continue
        if "start" not in r or "duration" not in r:
            continue
        hitches.append((int(r["start"][0]), int(r["duration"][0]), r.get("narrative-description", (None, ""))[1] or ""))
    # a hitch or phase in two tables is one
    hitches = sorted(set(hitches))
    phases = sorted(set(phases))
    print("| Phase | Seconds | Hitches | Hitch time (ms) | Hitch ms per s | Longest (ms) | Reasons |")
    print("|---|---|---|---|---|---|---|")
    for t0, t1, label in sorted(phases):
        hs = [h for h in hitches if t0 <= h[0] < t1]
        tot = sum(h[1] for h in hs) / 1e6
        secs = (t1 - t0) / 1e9
        reasons = sorted({re.sub(r",? ?\d+ offscreen passes\)?", "", h[2]).strip(", ") for h in hs if h[2]})
        print(f"| {label} | {secs:.1f} | {len(hs)} | {tot:.1f} | {tot / secs:.1f} | {max((h[1] for h in hs), default=0) / 1e6:.1f} | {'; '.join(reasons)} |")
    outside = [h for h in hitches if not any(t0 <= h[0] < t1 for t0, t1, _ in phases)]
    print(f"\nhitches outside phases (builds, re-raster, launch): {len(outside)}, {sum(h[1] for h in outside) / 1e6:.1f} ms")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3])
