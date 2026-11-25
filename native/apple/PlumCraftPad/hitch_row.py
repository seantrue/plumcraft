"""One row of bench.sh trace-wires' table: a hitches.py table's hitch ms a second for each phase.

Usage: python3 hitch_row.py HITCHES.md CONFIGURATION RUN
"""
import sys

PHASES = ["idle", "pan", "pan_fit", "zoom", "drag"]


def main(path, config, run):
    vals = {}
    try:
        lines = open(path).read().splitlines()
    except OSError:
        lines = []
    for line in lines:
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        # | 500.pan | 4.0 | 12 | 63.0 | 15.7 | ... : the phase's label is "<nodes>.<phase>"
        if len(cells) >= 5 and "." in cells[0]:
            phase = cells[0].split(".", 1)[1]
            if phase in PHASES:
                vals[phase] = cells[4]
    print(f"| {config} | {run} | " + " | ".join(vals.get(p, "?") for p in PHASES) + " |")


if __name__ == "__main__":
    main(*sys.argv[1:4])
