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
timeout 900 xcodebuild -project PlumCraftMac.xcodeproj -scheme PlumCraftMac -configuration Release \
  -derivedDataPath build -destination 'platform=macOS,arch=arm64' build -quiet 2>&1 | grep -v 'DVTProvisioning\|IDERunDestination' || true
app="$here/build/Build/Products/Release/PlumCraft.app/Contents/MacOS/PlumCraft"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/plumcraft-mac-bench.XXXXXX")"
# a copy left running (a time limit, ^C) is quit by the PID it wrote
cleanup() {
  # every copy the app started for this status file wrote its pid (a tracer may start several)
  for pid in $(cat "$scratch/status.json.pids" 2>/dev/null); do
    if kill -0 "$pid" 2>/dev/null; then kill -9 "$pid"; echo "cleared a copy left running ($pid)"; fi
  done
  rm -rf "$scratch"
}
trap cleanup EXIT INT TERM
# results are named for the slice that took them (PREFIX, default s4)
p="${PREFIX:-s4}"
export PLUMCRAFT_EDITOR_PREFS="$scratch/prefs.json" COMMONKIT_HASH=b3 COMMONKIT_STORE_MIN_FREE_GB=2 \
  COMMONKIT_STORE_ROOT="$scratch/store" COMMONKIT_MARK_DIR="$scratch/marks" COMMONKIT_TEST_RUN="$scratch/run" \
  PLUMCRAFT_CRAFT_ROOT="$scratch/craft"
args=(--bench "$out/$p-bench-$n.json" --nodes "$n" --wires "$m" --phase-seconds "$secs" --status-file "$scratch/status.json"
      --store-root "$scratch/store" --scratch-state --no-open-panel)
# EXTRA: more app arguments (say --engines-also none, to compare)
# shellcheck disable=SC2206
args+=(${EXTRA:-})
case "$what" in
run)
  timeout 300 "$app" "${args[@]}" | grep -v '^BENCH ' || true
  echo "wrote $out/$p-bench-$n.json" ;;
trace)
  t="$scratch/trace-$n.trace"; marker="$scratch/marker"; touch "$marker"
  before=" $(pgrep -x PlumCraft | tr '\n' ' ' || true) "
  args[1]="$out/$p-bench-$n-traced.json"
  timeout 300 xcrun xctrace record --template 'Animation Hitches' --instrument 'Points of Interest' \
    --time-limit 90s --output "$t" --launch -- "$app" "${args[@]}" | grep -v '^BENCH ' || true
  # xctrace may leave a copy behind, stopped before it ran a line (so it wrote no pid): any process
  # named PlumCraft (by exact name, never by command text) that wasn't there before the trace and
  # runs this build's binary is ours
  for pid in $(pgrep -x PlumCraft || true); do
    case " $before " in *" $pid "*) continue ;; esac
    if [ "$(ps -o comm= -p "$pid" 2>/dev/null)" = "$app" ]; then kill -9 "$pid" 2>/dev/null && echo "cleared a copy the tracer left ($pid)"; fi
  done
  # a stalled save leaves its raw kernel trace in $TMPDIR: remove this run's
  find "${TMPDIR:-/tmp}" -maxdepth 1 -name 'instruments*.ktrace' -newer "$marker" -delete 2>/dev/null || true
  for s in os-signpost hitches; do
    xcrun xctrace export --input "$t" --xpath "/trace-toc/run[@number=\"1\"]/data/table[@schema=\"$s\"]" > "$scratch/$s.xml" || true
  done
  python3 "$out/s3-hitches.py" PlumCraft "$scratch/os-signpost.xml" "$scratch/hitches.xml" | tee "$out/$p-trace-$n-hitches.md" ;;
*) echo "usage: $0 run|trace [nodes wires]"; exit 2 ;;
esac
