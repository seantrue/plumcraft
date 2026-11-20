#!/bin/bash
# The iPad app's budgets (plan section 10, read as N0's 15.2 does) on a real scene the core builds.
# Release builds. Local runs only (no macOS CI).
#
#   ./bench.sh sim [N M]           in the booted iPad Pro 13-inch (M5) simulator (its timings are the
#                                  Mac's, not an iPad's: for the run's shape only)
#   ./bench.sh device [N M]        on a connected, unlocked iPad (DEVICE=<udid>, default the first):
#                                  build, sign with the team's development identity and the profile
#                                  Xcode already has for it (automatic signing without
#                                  -allowProvisioningUpdates: the team's wildcard development profile;
#                                  nothing is requested), install as com.plumcraft.pad, launch the run,
#                                  copy its JSON back, and make sure it has quit
#   ./bench.sh trace-device [N M]  the same under Instruments' Animation Hitches on the device, hitches
#                                  binned by phase
#
# Results: claude-debug/native-n3/PREFIX-pad-<where>-N.json (PREFIX default s5). Nothing here asks
# Apple for a certificate or profile: a build that can't be signed with what is installed stops.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
out="$root/claude-debug/native-n3"
what="${1:-sim}"; n="${2:-500}"; m="${3:-800}"; secs="${PHASE_SECONDS:-4}"; p="${PREFIX:-s5}"
bundle=com.plumcraft.pad
team="${TEAM:-78DGVG7MYU}"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/plumcraft-pad-bench.XXXXXX")"
sim=""; dev=""
cleanup() {
  [ -n "$sim" ] && xcrun simctl terminate "$sim" "$bundle" 2>/dev/null || true
  if [ -n "$dev" ]; then
    # the app on the device, by its pid if it is still running
    pid=$(xcrun devicectl device info processes --device "$dev" --json-output "$scratch/ps.json" >/dev/null 2>&1 &&
          python3 -c "
import json
for q in json.load(open('$scratch/ps.json'))['result']['runningProcesses']:
    if q.get('executable','').endswith('/PlumCraftPad.app/PlumCraftPad'): print(q['processIdentifier'])" 2>/dev/null | head -1 || true)
    [ -n "$pid" ] && xcrun devicectl device process terminate --device "$dev" --pid "$pid" >/dev/null 2>&1 && echo "quit the app on the device ($pid)"
  fi
  rm -rf "$scratch"
}
trap cleanup EXIT INT TERM
export PLUMCRAFT_EDITOR_PREFS="$scratch/prefs.json" COMMONKIT_HASH=b3 COMMONKIT_STORE_MIN_FREE_GB=2 \
  COMMONKIT_STORE_ROOT="$scratch/store" COMMONKIT_MARK_DIR="$scratch/marks" COMMONKIT_TEST_RUN="$scratch/run" \
  PLUMCRAFT_CRAFT_ROOT="$scratch/craft"
args=(--bench "bench-$n.json" --nodes "$n" --wires "$m" --phase-seconds "$secs" --scratch-state)
cd "$here"
command -v xcodegen >/dev/null && xcodegen generate --quiet
case "$what" in
sim)
  sim="${SIM:-$(xcrun simctl list devices booted | grep 'iPad Pro 13-inch (M5)' | head -1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')}"
  timeout 900 xcodebuild -project PlumCraftPad.xcodeproj -scheme PlumCraftPad -configuration Release -derivedDataPath build \
    -destination "id=$sim" build -quiet 2>&1 | grep -v 'appintentsmetadataprocessor\|DVTProvisioning' || true
  xcrun simctl install "$sim" build/Build/Products/Release-iphonesimulator/PlumCraftPad.app
  timeout 300 xcrun simctl launch --console-pty --terminate-running-process "$sim" "$bundle" "${args[@]}" | grep -v '^BENCH ' || true
  cp "$(xcrun simctl get_app_container "$sim" "$bundle" data)/Documents/bench-$n.json" "$out/$p-pad-sim-$n.json"
  echo "wrote $out/$p-pad-sim-$n.json" ;;
device|trace-device)
  dev="${DEVICE:-$(xcrun devicectl list devices 2>/dev/null | grep -E 'iPad.*physical' | grep -E 'available|connected' | head -1 | grep -oE '[0-9A-F]{8}-[0-9A-F]{16}')}"
  [ -n "$dev" ] || { echo "no iPad connected"; exit 2; }
  # signed with what is installed: the team's development identity and an existing profile
  timeout 900 xcodebuild -project PlumCraftPad.xcodeproj -target PlumCraftPad -configuration Release -sdk iphoneos \
    SYMROOT="$here/build-device" OBJROOT="$here/build-device/obj" \
    CODE_SIGN_STYLE=Automatic DEVELOPMENT_TEAM="$team" CODE_SIGN_IDENTITY="Apple Development" \
    build -quiet 2>&1 | grep -v 'appintentsmetadataprocessor\|DVTProvisioning' || true
  app="$here/build-device/Release-iphoneos/PlumCraftPad.app"
  [ -d "$app" ] || { echo "the device build failed (signing?)"; exit 1; }
  timeout 300 xcrun devicectl device install app --device "$dev" "$app" >/dev/null
  if [ "$what" = device ]; then
    timeout 300 xcrun devicectl device process launch --device "$dev" --terminate-existing --console "$bundle" "${args[@]}" | grep -v '^BENCH ' || true
    xcrun devicectl device copy from --device "$dev" --domain-type appDataContainer --domain-identifier "$bundle" \
      --source "Documents/bench-$n.json" --destination "$out/$p-pad-device-$n.json" >/dev/null
    echo "wrote $out/$p-pad-device-$n.json"
  else
    t="$scratch/trace.trace"
    timeout 300 xcrun xctrace record --device "$dev" --template 'Animation Hitches' --instrument 'Points of Interest' \
      --time-limit 90s --output "$t" --launch -- "$bundle" "${args[@]}" >/dev/null 2>&1 || true
    for s in os-signpost hitches; do
      xcrun xctrace export --input "$t" --xpath "/trace-toc/run[@number=\"1\"]/data/table[@schema=\"$s\"]" > "$scratch/$s.xml" || true
    done
    python3 "$out/s5-hitches.py" PlumCraft "$scratch/os-signpost.xml" "$scratch/hitches.xml" | tee "$out/$p-pad-device-trace-$n-hitches.md"
  fi ;;
*) echo "usage: $0 sim|device|trace-device [nodes wires]"; exit 2 ;;
esac
