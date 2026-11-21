#!/bin/bash
# The iPad app's budgets (plan section 10, read as N0's 15.2 does) on a real scene the core builds.
# Release builds. Local runs only (no macOS CI).
#
#   ./bench.sh sim [N M]           in the booted iPad Pro 13-inch (M5) simulator (its timings are the
#                                  Mac's, not an iPad's: for the run's shape only)
#   ./bench.sh device [N M]        on a connected, unlocked iPad (DEVICE=<udid>, default the first):
#                                  build, sign with the team's development identity and the profile
#                                  Xcode already has for it (automatic signing without
#                                  -allowProvisioningUpdates: nothing is requested), install as
#                                  the app's bundle id (Config/BundleId.xcconfig), launch the run, copy its JSON back, and make sure
#                                  it has quit
#   ./bench.sh trace-device [N M]  the same under Instruments' Animation Hitches on the device, hitches
#                                  binned by phase (hitches.py)
#   ./bench.sh trace-spike-device [N M]  the N0 spike (already installed) under the same trace: the
#                                  control
#
# BENCH_EXTRA adds bench arguments (--hide-wires: no wires drawn, the faces' share of a trace).
# APP=<path to a built Plummer.app> runs that app instead of building this checkout (a device
# app for device and trace-device, a simulator app for sim). OUT names the results' folder (default
# claude-debug/native-n3), PREFIX their names' start (default s5): OUT/PREFIX-pad-<where>-N.json and,
# for a trace, OUT/PREFIX-<app>-device-trace-N-hitches.md beside its exported tables and xctrace log.
#
# Never an old result (the N4 device gate): each run's JSON has a name of its own, the app removes
# earlier runs' files when it starts, a result already in OUT under the name about to be written
# is removed first, and a launch the iPad refuses (locked, or not unlocked since it restarted) stops
# the run with exit 5 and reads nothing. Exit 4: another launch failure; 2: no iPad.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
out="${OUT:-$root/claude-debug/native-n3}"; mkdir -p "$out"; out="$(cd "$out" && pwd)"
what="${1:-sim}"; n="${2:-500}"; m="${3:-800}"; secs="${PHASE_SECONDS:-4}"; p="${PREFIX:-s5}"
bundle=$(sed -n 's/^PLUMMER_PAD_BUNDLE_ID = //p' "$here/../Config/BundleId.xcconfig")
team="${TEAM:-}"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/plumcraft-pad-bench.XXXXXX")"
sim=""; dev=""
cleanup() {
  [ -n "$sim" ] && xcrun simctl terminate "$sim" "$bundle" 2>/dev/null || true
  if [ -n "$dev" ]; then
    # the bench apps on the device, by their pids, if still running
    pid=$(timeout 60 xcrun devicectl device info processes --device "$dev" --json-output "$scratch/ps.json" >/dev/null 2>&1 &&
          python3 -c "
import json
for q in json.load(open('$scratch/ps.json'))['result']['runningProcesses']:
    e = q.get('executable','')
    if e.endswith('/Plummer.app/Plummer') or e.endswith('/CanvasSpikePad.app/CanvasSpikePad'): print(q['processIdentifier'])" 2>/dev/null || true)
    for q in $pid; do
      timeout 60 xcrun devicectl device process terminate --device "$dev" --pid "$q" >/dev/null 2>&1 && echo "quit a bench app on the device ($q)"
    done
  fi
  rm -rf "$scratch"
}
trap cleanup EXIT INT TERM
export PLUMCRAFT_EDITOR_PREFS="$scratch/prefs.json" COMMONKIT_HASH=b3 COMMONKIT_STORE_MIN_FREE_GB=2 \
  COMMONKIT_STORE_ROOT="$scratch/store" COMMONKIT_MARK_DIR="$scratch/marks" COMMONKIT_TEST_RUN="$scratch/run" \
  PLUMCRAFT_CRAFT_ROOT="$scratch/craft"
# APP given relative to where this was run from (the script moves to its own folder below)
[ -n "${APP:-}" ] && [ "${APP#/}" = "$APP" ] && APP="$PWD/$APP"
run_id="$(date +%H%M%S)-$$"
result="bench-$n-$run_id.json"
args=(--bench "$result" --nodes "$n" --wires "$m" --phase-seconds "$secs" --scratch-state)
# BENCH_EXTRA: more bench arguments (`--hide-wires`: the faces alone)
[ -n "${BENCH_EXTRA:-}" ] && args+=(${BENCH_EXTRA})
# a launch's output said the iPad refused it because it is locked
locked() { grep -qE 'BSErrorCodeDescription = Locked|could not be, unlocked|device is locked|Locked \(' "$1"; }
# A recording on the device of app BUNDLE (process name PROC) launched with the rest, its hitches
# binned by phase into NAME-hitches.md. xctrace's own output goes to NAME-xctrace.log and the two
# exported tables beside it (NAME-os-signpost.xml, NAME-hitches.xml). The recording stops when the
# app exits (each bench exits once done), or at 120 s. A trace with no phases is not a result.
#
# xctrace writes a raw kernel trace (instruments*.ktrace, 2 to 8 GB) into TMPDIR and leaves it there
# when a save stalls; on 2026-11-21 fifteen of them filled the Mac's disk. So each recording gets a
# folder of its own under this run's scratch, given to xctrace alone as TMPDIR, and removed (with
# the trace) once the tables are exported, or by the EXIT trap on a failure, a time limit or ^C; and
# no recording starts with less than MIN_FREE_GB (15) free where the scratch is.
min_free_gb="${MIN_FREE_GB:-15}"
free_gb() { df -Pk "$1" | awk 'NR == 2 { print int($4 / 1048576) }'; }
trace_device() {
  local b="$1" proc="$2" name="$3"; shift 3
  local free
  free=$(free_gb "$scratch")
  if [ "$free" -lt "$min_free_gb" ]; then
    echo "only $free GB free on the scratch's disk: a trace needs $min_free_gb GB (xctrace's raw trace runs to 8 GB); not started"
    exit 6
  fi
  # one folder per recording, short, under the scratch the trap removes
  local xt; xt="$(mktemp -d "$scratch/x.XXXX")"
  local t="$xt/t.trace"
  rm -f "$out/$name-hitches.md" "$out/$name-os-signpost.xml" "$out/$name-hitches.xml" "$out/$name-xctrace.log"
  TMPDIR="$xt/" timeout 400 xcrun xctrace record --device "$dev" --template 'Animation Hitches' --instrument 'Points of Interest' \
    --time-limit 120s --output "$t" --launch -- "$b" "$@" > "$out/$name-xctrace.log" 2>&1 || echo "xctrace record: exit $?" >> "$out/$name-xctrace.log"
  if locked "$out/$name-xctrace.log"; then echo "the iPad is locked: the trace didn't launch (unlock it; Auto-Lock to Never)"; exit 5; fi
  [ -d "$t" ] || { echo "no trace was saved ($out/$name-xctrace.log)"; exit 4; }
  for s in os-signpost hitches; do
    TMPDIR="$xt/" timeout 300 xcrun xctrace export --input "$t" --xpath "/trace-toc/run[@number=\"1\"]/data/table[@schema=\"$s\"]" \
      > "$out/$name-$s.xml" 2>> "$out/$name-xctrace.log" || echo "xctrace export $s: exit $?" >> "$out/$name-xctrace.log"
  done
  # the trace and any raw kernel trace xctrace left: gone now the tables are out
  rm -rf "$xt"
  python3 "$here/hitches.py" "$proc" "$out/$name-os-signpost.xml" "$out/$name-hitches.xml" | tee "$out/$name-hitches.md"
  grep -q '^| [^P-][^|]* | [0-9]' "$out/$name-hitches.md" || { echo "the trace has no phases: not a result"; exit 4; }
}
cd "$here"
command -v xcodegen >/dev/null && xcodegen generate --quiet
case "$what" in
sim)
  sim="${SIM:-$(xcrun simctl list devices booted | grep 'iPad Pro 13-inch (M5)' | head -1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')}"
  app="${APP:-}"
  if [ -z "$app" ]; then
    timeout 900 xcodebuild -project PlumCraftPad.xcodeproj -scheme PlumCraftPad -configuration Release -derivedDataPath build \
      -destination "id=$sim" build -quiet 2>&1 | grep -v 'appintentsmetadataprocessor\|DVTProvisioning' || true
    app=build/Build/Products/Release-iphonesimulator/Plummer.app
  fi
  xcrun simctl install "$sim" "$app"
  dest="$out/$p-pad-sim-$n.json"; rm -f "$dest"
  timeout 300 xcrun simctl launch --console-pty --terminate-running-process "$sim" "$bundle" "${args[@]}" | grep -v '^BENCH ' || true
  cp "$(xcrun simctl get_app_container "$sim" "$bundle" data)/Documents/$result" "$dest"
  echo "wrote $dest" ;;
device|trace-device)
  dev="${DEVICE:-$(xcrun devicectl list devices 2>/dev/null | grep -E 'iPad.*physical' | grep -E 'available|connected' | head -1 | grep -oE '[0-9A-F]{8}-[0-9A-F]{16}')}"
  [ -n "$dev" ] || { echo "no iPad connected"; exit 2; }
  app="${APP:-}"
  if [ -z "$app" ]; then
    [ -n "$team" ] || { echo "set TEAM to the signing team's id (or APP to a built app)"; exit 2; }
    # signed with what is installed: the team's development identity and an existing profile
    timeout 900 xcodebuild -project PlumCraftPad.xcodeproj -target PlumCraftPad -configuration Release -sdk iphoneos \
      SYMROOT="$here/build-device" OBJROOT="$here/build-device/obj" \
      CODE_SIGN_STYLE=Automatic DEVELOPMENT_TEAM="$team" CODE_SIGN_IDENTITY="Apple Development" \
      build -quiet 2>&1 | grep -v 'appintentsmetadataprocessor\|DVTProvisioning' || true
    app="$here/build-device/Release-iphoneos/Plummer.app"
  fi
  [ -d "$app" ] || { echo "no device app at $app (the build failed: signing?)"; exit 1; }
  timeout 300 xcrun devicectl device install app --device "$dev" "$app" >"$scratch/install.log" 2>&1 || { cat "$scratch/install.log"; exit 4; }
  if [ "$what" = device ]; then
    dest="$out/$p-pad-device-$n.json"; rm -f "$dest"
    set +e
    timeout 300 xcrun devicectl device process launch --device "$dev" --terminate-existing --console "$bundle" "${args[@]}" >"$scratch/launch.log" 2>&1
    code=$?
    set -e
    grep -v '^BENCH ' "$scratch/launch.log" | grep -v 'keyboard extensions' || true
    if locked "$scratch/launch.log"; then echo "the iPad is locked: the launch was refused, no result read (unlock it; Auto-Lock to Never)"; exit 5; fi
    [ "$code" = 0 ] || { echo "the launch failed (exit $code): no result read"; exit 4; }
    timeout 120 xcrun devicectl device copy from --device "$dev" --domain-type appDataContainer --domain-identifier "$bundle" \
      --source "Documents/$result" --destination "$dest" >/dev/null 2>&1 || { echo "this run left no result ($result): none read"; exit 4; }
    echo "wrote $dest"
  else
    trace_device "$bundle" Plummer "$p-pad-device-trace-$n" "${args[@]}"
  fi ;;
trace-spike-device)
  # the N0 spike (com.plumcraft.spike.canvas, installed at N0 and left as it is) under the same
  # trace: the control beside the app's, on the same device in the same minutes
  dev="${DEVICE:-$(xcrun devicectl list devices 2>/dev/null | grep -E 'iPad.*physical' | grep -E 'available|connected' | head -1 | grep -oE '[0-9A-F]{8}-[0-9A-F]{16}')}"
  [ -n "$dev" ] || { echo "no iPad connected"; exit 2; }
  trace_device com.plumcraft.spike.canvas CanvasSpike "$p-spike-device-trace-$n" --bench "bench-$n-$run_id.json" --nodes "$n" --wires "$m" \
    --phase-seconds "$secs" --modes perWire ;;
*) echo "usage: $0 sim|device|trace-device|trace-spike-device [nodes wires]"; exit 2 ;;
esac
