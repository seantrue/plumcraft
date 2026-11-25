#!/bin/bash
# The Mac app's budgets (plan section 10, read as N0's 15.2 does) on a real scene the core builds.
# Release build, scratch preferences and store. Local runs only (no macOS CI).
#   ./bench.sh run [N M]     the scripted run: claude-debug/native-n3/PREFIX-bench-N.json
#   ./bench.sh trace [N M]   the same under Instruments' Animation Hitches, hitches binned by phase
#                            (claude-debug/native-n3/PREFIX-trace-N-hitches.md); the raw trace is removed
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
out="$root/claude-debug/native-n3"
mkdir -p "$out"
what="${1:-run}"; n="${2:-500}"; m="${3:-800}"; secs="${PHASE_SECONDS:-4}"
cd "$here"
command -v xcodegen >/dev/null && xcodegen generate --quiet
# DERIVED: another build folder (a copy of the app running from build/ is left alone)
derived="${DERIVED:-build}"
case "$derived" in /*) ;; *) derived="$here/$derived" ;; esac
timeout 900 xcodebuild -project PlumCraftMac.xcodeproj -scheme PlumCraftMac -configuration Release \
  -derivedDataPath "$derived" -destination 'platform=macOS,arch=arm64' build -quiet 2>&1 | grep -v 'DVTProvisioning\|IDERunDestination' || true
scratch="$(mktemp -d "${TMPDIR:-/tmp}/plumcraft-mac-bench.XXXXXX")"
copy=""
# a copy left running (a time limit, ^C) is quit by the PID it wrote; then the person's folders
# are compared with how they were (a change fails the run loudly) and the scripted copy goes
cleanup() {
  local code=$?
  # every copy the app started for this status file wrote its pid (a tracer may start several)
  for pid in $(cat "$scratch/status.json.pids" 2>/dev/null); do
    if kill -0 "$pid" 2>/dev/null; then kill -9 "$pid"; echo "cleared a copy left running ($pid)"; fi
  done
  if [ -f "$scratch/home-before.txt" ]; then
    "$here/library-check.sh" home "$scratch/home-after.txt"
    "$here/library-check.sh" home-compare "$scratch/home-before.txt" "$scratch/home-after.txt" || code=1
  fi
  [ -n "$copy" ] && "$here/scripted-app.sh" clean "$copy"
  # B76: a recording's raw kernel trace, which DTServiceHub leaves in the per-user temporary folder
  [ -f "$scratch/ktrace-before" ] && "$here/../ktrace-sweep.sh" remove "$scratch/ktrace-before"
  rm -rf "$scratch"
  exit $code
}
trap cleanup EXIT INT TERM
# B84: a scripted launch runs as a copy under the scripted bundle id (scripted-app.sh)
person_bundle="$derived/Build/Products/Release/Plummer.app"
person_id=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$person_bundle/Contents/Info.plist")
"$here/scripted-app.sh" clean "$person_id.scripted"
copy=$("$here/scripted-app.sh" make "$person_bundle" "$scratch/app") || { echo "could not make the scripted copy"; exit 2; }
app="$copy/Contents/MacOS/Plummer"
"$here/library-check.sh" home "$scratch/home-before.txt"
# results are named for the slice that took them (PREFIX, default s4)
p="${PREFIX:-s4}"
export PLUMCRAFT_EDITOR_PREFS="$scratch/prefs.json" COMMONKIT_HASH=b3 COMMONKIT_STORE_MIN_FREE_GB=2 \
  COMMONKIT_STORE_ROOT="$scratch/store" COMMONKIT_MARK_DIR="$scratch/marks" COMMONKIT_TEST_RUN="$scratch/run" \
  PLUMCRAFT_CRAFT_ROOT="$scratch/craft" PLUMCRAFT_ENGINE_FOLDER="$scratch/engines"
# a scripted launch refuses without an engines folder of its own (decision 7): an empty one
mkdir -p "$scratch/engines"
args=(--bench "$out/$p-bench-$n.json" --nodes "$n" --wires "$m" --phase-seconds "$secs" --status-file "$scratch/status.json"
      --store-root "$scratch/store" --scratch-state --no-open-panel)
# EXTRA: more app arguments (say --engines-also none, to compare)
# shellcheck disable=SC2206
args+=(${EXTRA:-})
case "$what" in
run)
  # WRAP: a command to run the app under, to see how the run moves when the system demotes it
  # (say `taskpolicy -c utility`); none by default
  # shellcheck disable=SC2206
  wrap=(${WRAP:-})
  timeout 300 ${wrap[@]+"${wrap[@]}"} "$app" "${args[@]}" | grep -v '^BENCH ' || true
  echo "wrote $out/$p-bench-$n.json" ;;
trace)
  # xctrace writes a raw kernel trace (instruments*.ktrace, 2 to 8 GB) into TMPDIR and leaves it when
  # a save stalls (on 2026-11-21 fifteen of them filled the disk): the recording gets a folder of
  # its own under the scratch, given to xctrace alone as TMPDIR, removed with the scratch by the
  # EXIT trap (a failure, a time limit, ^C) or once the tables are out; and no recording starts
  # with less than MIN_FREE_GB (15) free where the scratch is.
  free=$(df -Pk "$scratch" | awk 'NR == 2 { print int($4 / 1048576) }')
  if [ "$free" -lt "${MIN_FREE_GB:-15}" ]; then
    echo "only $free GB free on the scratch's disk: a trace needs ${MIN_FREE_GB:-15} GB (xctrace's raw trace runs to 8 GB); not started"
    exit 6
  fi
  xt="$(mktemp -d "$scratch/x.XXXX")"
  t="$xt/t.trace"; marker="$scratch/marker"; touch "$marker"
  before=" $(pgrep -x Plummer | tr '\n' ' ' || true) "
  args[1]="$out/$p-bench-$n-traced.json"
  # B76: the raw kernel trace is DTServiceHub's, in the per-user temporary folder whatever TMPDIR
  # says; the ones there now are listed, and every new one removed when the tables are out
  "$here/../ktrace-sweep.sh" list > "$scratch/ktrace-before"
  TMPDIR="$xt/" timeout 300 xcrun xctrace record --template 'Animation Hitches' --instrument 'Points of Interest' \
    --time-limit 90s --output "$t" --launch -- "$app" "${args[@]}" | grep -v '^BENCH ' || true
  # xctrace may leave a copy behind, stopped before it ran a line (so it wrote no pid): any process
  # named Plummer (by exact name, never by command text) that wasn't there before the trace and
  # runs this build's binary is ours. So is one stopped (state T) that runs another build of this
  # checkout: the system can resolve the tracer's extra launch to the newest Plummer.app it knows
  # (the budget stage found two left by a clean build's bundle). A copy someone starts meanwhile
  # runs, so it isn't stopped.
  for pid in $(pgrep -x Plummer || true); do
    case " $before " in *" $pid "*) continue ;; esac
    comm="$(ps -o comm= -p "$pid" 2>/dev/null)"; state="$(ps -o stat= -p "$pid" 2>/dev/null)"
    case "$comm" in
      "$app") ;;
      "$root"/*/Plummer.app/Contents/MacOS/Plummer) case "$state" in T*) ;; *) continue ;; esac ;;
      *) continue ;;
    esac
    kill -9 "$pid" 2>/dev/null && echo "cleared a copy the tracer left ($pid)"
  done
  for s in os-signpost hitches; do
    TMPDIR="$xt/" timeout 300 xcrun xctrace export --input "$t" --xpath "/trace-toc/run[@number=\"1\"]/data/table[@schema=\"$s\"]" > "$scratch/$s.xml" || true
  done
  # the trace, and the raw kernel trace DTServiceHub left in the per-user temporary folder: gone
  # now the tables are out
  rm -rf "$xt"
  "$here/../ktrace-sweep.sh" remove "$scratch/ktrace-before"
  # the exports are kept beside the table (a trace with no phases says why), read by s6-hitches.py,
  # which reads every table of an export (s3-hitches.py read only the fullest)
  for s in os-signpost hitches; do cp "$scratch/$s.xml" "$out/$p-trace-$n-$s.xml" 2>/dev/null || true; done
  python3 "$out/s6-hitches.py" Plummer "$scratch/os-signpost.xml" "$scratch/hitches.xml" | tee "$out/$p-trace-$n-hitches.md" ;;
*) echo "usage: $0 run|trace [nodes wires]"; exit 2 ;;
esac
