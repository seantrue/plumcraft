#!/bin/bash
# B76: Instruments' raw kernel trace (instruments*.ktrace, 2 to 8 GB a trace on a device) is
# written by Xcode's DTServiceHub, a service launchd starts, into the per-user temporary folder
# (`getconf DARWIN_USER_TEMP_DIR`, /var/folders/…/T/), not into the TMPDIR given to xctrace, and
# is left there when the recording ends (it stays open in DTServiceHub, which lingers). Found
# 2026-11-21 with a three-second System Trace of a Mac process: a 386 MB instruments*.ktrace
# appeared there, nothing in xctrace's TMPDIR. So a script that records:
#
#   ktrace-sweep.sh list > BEFORE      the raw traces there now (name a line), before it records
#   ktrace-sweep.sh remove BEFORE      after the recording (and on any way out): every raw trace
#                                      there that is not in BEFORE, of this user, removed; prints
#                                      what went and its size
#
# A trace another program started in the same minutes would be taken for this one's: the bench
# scripts record one trace at a time and say what they removed.
set -uo pipefail
dir="$(getconf DARWIN_USER_TEMP_DIR)"
list() { find "$dir" -maxdepth 1 -name 'instruments*.ktrace' -user "$(id -u)" -print 2>/dev/null | sed 's|.*/||' | sort; }
case "${1:-}" in
list) list ;;
remove)
  before="${2:?remove BEFORE}"
  [ -f "$before" ] || { echo "ktrace-sweep: no list $before: nothing removed" >&2; exit 0; }
  list | comm -13 "$before" - | while IFS= read -r f; do
    [ -n "$f" ] || continue
    p="$dir$f"
    [ -f "$p" ] || continue
    echo "removed Instruments' raw trace $p ($(stat -f '%z' "$p") bytes)"
    rm -f "$p"
  done ;;
*) echo "usage: $0 list | remove BEFORE" >&2; exit 2 ;;
esac
