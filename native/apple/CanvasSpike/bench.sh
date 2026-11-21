#!/bin/bash
# N0(a) canvas spike: build and benchmark. Local runs only (no CI).
#   ./bench.sh mac [N M]        Mac app, Release, writes ../../../claude-debug/native-n0/mac-N.json
#   ./bench.sh sim [N M]        iPad Pro 13-inch (M5) simulator (timing not representative)
#   ./bench.sh device [N M]     a physical iPad (first available one, or DEVICE=<udid>), signed with
#                               the team in PLUMCRAFT_TEAM_ID
#   ./bench.sh trace [N M]      Mac bench under Instruments' Animation Hitches; hitches binned by phase
#   ./bench.sh shot             a screenshot of the Mac app
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="$(cd "$here/../../.." && pwd)/claude-debug/native-n0"
mkdir -p "$out"
cd "$here"
command -v xcodegen >/dev/null && xcodegen generate --quiet
what="${1:-mac}"; n="${2:-500}"; m="${3:-800}"
secs="${PHASE_SECONDS:-4}"
bench_args=(--bench "bench-$n.json" --nodes "$n" --wires "$m" --phase-seconds "$secs")
case "$what" in
mac|shot|trace)
  xcodebuild -project CanvasSpike.xcodeproj -scheme CanvasSpikeMac -configuration Release \
    -derivedDataPath build -destination 'platform=macOS,arch=arm64' build -quiet
  app="build/Build/Products/Release/CanvasSpikeMac.app/Contents/MacOS/CanvasSpikeMac"
  if [ "$what" = trace ]; then
    # one short recording per wire mode: xctrace stalled saving recordings much over a minute
    IFS=, read -ra modes <<<"${MODES:-perWire,perWireLoose,batched,hybrid}"
    for mode in "${modes[@]}"; do
      t="$out/canvas-mac-$n-$mode.trace"; rm -rf "$t"
      marker="$(mktemp)"
      timeout 200 xcrun xctrace record --template 'Animation Hitches' --instrument 'Points of Interest' \
        --time-limit 60s --output "$t" --launch -- "$PWD/$app" \
        --bench "$out/mac-$n-traced-$mode.json" --nodes "$n" --wires "$m" --phase-seconds "$secs" \
        --modes "$mode" | grep -v '^BENCH_JSON' || true
      # xctrace's --launch leaves a stopped (state T) copy of the app behind; clear it
      pkill -9 -f "CanvasSpikeMac.app/Contents/MacOS/CanvasSpikeMac --bench" || true
      # a stalled save leaves its raw kernel trace (5-60 GB) in $TMPDIR: remove this run's
      find "${TMPDIR:-/tmp}" -maxdepth 1 -name 'instruments*.ktrace' -newer "$marker" -delete 2>/dev/null || true
      rm -f "$marker"
      for s in os-signpost hitches; do
        xcrun xctrace export --input "$t" --xpath "/trace-toc/run[@number=\"1\"]/data/table[@schema=\"$s\"]" > "$out/canvas-mac-$n-$mode-$s.xml" || true
      done
      python3 "$out/../native_n0_hitches.py" "$out/canvas-mac-$n-$mode-os-signpost.xml" "$out/canvas-mac-$n-$mode-hitches.xml" | tee "$out/canvas-mac-$n-$mode-hitches.md"
    done
  elif [ "$what" = shot ]; then
    # the window as the window server composited it (needs Screen Recording for the terminal)
    for z in fit 1; do
      log="$(mktemp)"; zarg=(); [ "$z" = fit ] || zarg=(--zoom "$z")
      "$app" "${zarg[@]}" >"$log" 2>&1 &
      pid=$!; for _ in $(seq 30); do grep -q '^window:' "$log" && break; sleep 0.2; done; sleep 1.5
      screencapture -x -o -l"$(awk '/^window:/{print $2}' "$log")" "$out/mac-window-zoom$z.png"
      kill "$pid"; rm -f "$log"; echo "wrote $out/mac-window-zoom$z.png"
    done
  else
    "$app" --bench "$out/mac-$n.json" --nodes "$n" --wires "$m" --phase-seconds "$secs" ${MODES:+--modes "$MODES"} | grep -v '^BENCH_JSON'
  fi
  ;;
sim)
  udid="${SIM:-$(xcrun simctl list devices available | grep 'iPad Pro 13-inch (M5)' | head -1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')}"
  xcrun simctl boot "$udid" 2>/dev/null || true
  xcodebuild -project CanvasSpike.xcodeproj -scheme CanvasSpikePad -configuration Release \
    -derivedDataPath build -destination "id=$udid" build -quiet CODE_SIGNING_ALLOWED=NO
  xcrun simctl install "$udid" build/Build/Products/Release-iphonesimulator/CanvasSpikePad.app
  xcrun simctl terminate "$udid" com.plumcraft.spike.canvas 2>/dev/null || true
  xcrun simctl launch --console-pty "$udid" com.plumcraft.spike.canvas "${bench_args[@]}" | grep -v '^BENCH_JSON' || true
  cp "$(xcrun simctl get_app_container "$udid" com.plumcraft.spike.canvas data)/Documents/bench-$n.json" "$out/sim-$n.json"
  echo "wrote $out/sim-$n.json"
  ;;
device)
  : "${PLUMCRAFT_TEAM_ID:?set PLUMCRAFT_TEAM_ID to the signing team id}"
  udid="${DEVICE:-$(xcrun devicectl list devices 2>/dev/null | grep -E 'iPad.*physical' | grep available | head -1 | awk '{for(i=1;i<=NF;i++) if ($i ~ /^[0-9A-F]{8}-[0-9A-F]{16}$/) print $i}')}"
  xcodebuild -project CanvasSpike.xcodeproj -scheme CanvasSpikePad -configuration Release \
    -derivedDataPath build -destination "id=$udid" -allowProvisioningUpdates build -quiet
  xcrun devicectl device install app --device "$udid" build/Build/Products/Release-iphoneos/CanvasSpikePad.app
  xcrun devicectl device process launch --device "$udid" --terminate-existing --console \
    com.plumcraft.spike.canvas "${bench_args[@]}" | grep -v '^BENCH_JSON' || true
  xcrun devicectl device copy from --device "$udid" --domain-type appDataContainer \
    --domain-identifier com.plumcraft.spike.canvas --source "Documents/bench-$n.json" --destination "$out/device-$n.json"
  echo "wrote $out/device-$n.json"
  ;;
*) echo "usage: $0 mac|sim|device|shot [nodes wires]"; exit 2 ;;
esac
