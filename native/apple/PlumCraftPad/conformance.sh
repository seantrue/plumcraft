#!/bin/bash
# The conformance suite's read-only half (docs/shell-interface.md 16.1) against the iPad app running
# in the simulator, which measures its own faces: build the app (release) and the runner, install
# the app in the booted simulator, launch it on scratch state with its control port open (loopback,
# which the simulator shares with the Mac) and an empty graph, run the cases over the port, and
# terminate the app. The app's store, marks and preferences are inside its own container (the app
# sets them itself); nothing it writes reaches the Mac's store. Local runs only (no macOS CI).
#
#   native/apple/PlumCraftPad/conformance.sh [read|edit|all|gestures]      (default read)
#
# `gestures` runs the UI test instead (UITests/GestureTests.swift): real taps, a long press's
# context menu, a pinch, a drag and a double tap through UIKit's recognisers, and the rotations.
# SIM names the simulator (default: the booted "iPad Pro 13-inch (M5)").
set -euo pipefail
half="${1:-read}"
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
kit="$here/../PlumCraftFFI"
bundle=$(sed -n 's/^PLUMMER_PAD_BUNDLE_ID = //p' "$here/../Config/BundleId.xcconfig")
sim="${SIM:-$(xcrun simctl list devices booted | grep 'iPad Pro 13-inch (M5)' | head -1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')}"
[ -n "$sim" ] || { echo "no booted iPad Pro 13-inch (M5) simulator (set SIM)"; exit 2; }
scratch="$(mktemp -d "${TMPDIR:-/tmp}/plumcraft-pad-conformance.XXXXXX")"
# the app goes on every way out (a failure, a time limit, ^C)
cleanup() {
  xcrun simctl terminate "$sim" "$bundle" 2>/dev/null || true
  rm -rf "$scratch"
}
trap cleanup EXIT INT TERM
# the build and the runner on scratch too (the login shell may name the Mac's own store)
export PLUMCRAFT_EDITOR_PREFS="$scratch/prefs.json" COMMONKIT_HASH=b3 COMMONKIT_STORE_MIN_FREE_GB=2 \
  COMMONKIT_STORE_ROOT="$scratch/store" COMMONKIT_MARK_DIR="$scratch/marks" COMMONKIT_TEST_RUN="$scratch/run" \
  PLUMCRAFT_CRAFT_ROOT="$scratch/craft"

cd "$here"
command -v xcodegen >/dev/null && xcodegen generate --quiet
if [ "$half" = gestures ]; then
  shots="${SHOTS:-$root/claude-debug/native-n3/s5-shots}"; mkdir -p "$shots"
  TEST_RUNNER_PLUMCRAFT_STATUS="$scratch/ui-status.json" TEST_RUNNER_PLUMCRAFT_SHOTS="$shots" \
    timeout 900 xcodebuild -project PlumCraftPad.xcodeproj -scheme PlumCraftPad -configuration Debug -derivedDataPath build \
    -destination "id=$sim" -resultBundlePath "$scratch/ui.xcresult" test 2>&1 | grep -E 'Test Case|error:|TEST (SUCCEEDED|FAILED)'
  exit "${PIPESTATUS[0]}"
fi
# a build that fails stops here (N4 slice 4: it used to go on, and ran the app installed before)
set +e
timeout 900 xcodebuild -project PlumCraftPad.xcodeproj -scheme PlumCraftPad -configuration Release \
  -derivedDataPath build -destination "id=$sim" build -quiet 2>&1 | grep -v 'appintentsmetadataprocessor'
built=${PIPESTATUS[0]}
set -e
[ "$built" = 0 ] || { echo "the app didn't build: stopped"; exit 1; }
(cd "$kit" && timeout 900 swift build -c release --product plumcraft-conformance -q)
runner="$kit/.build/release/plumcraft-conformance"
xcrun simctl install "$sim" "$here/build/Build/Products/Release-iphonesimulator/Plummer.app"
data="$(xcrun simctl get_app_container "$sim" "$bundle" data)"
t="$data/tmp/conformance"; rm -rf "$t"; mkdir -p "$t"
echo '{"version": 1, "nodes": {}, "wires": []}' > "$t/empty.json"
status="$t/status.json"
# every case on one app, as it opens every graph: the empty graph starts on the text engine alone,
# and the picture case's first array node starts the array engine (decision 8C: engines start as
# the graph comes to use them), so no case is skipped for want of an engine
xcrun simctl launch --terminate-running-process "$sim" "$bundle" --status-file "$status" --control-token-file "$t/token" \
  --scratch-state --hide-watched --engine text --open "$t/empty.json" >/dev/null
# wait (at most 60 s) for the port and the graph's canvas
doc=""
for _ in $(seq 600); do
  doc=$(python3 -c "
import json
s=json.load(open('$status')); w=s.get('windows',[])
print(w[0]['doc'] if s.get('port') and w and w[0].get('ready') else '')" 2>/dev/null || true)
  [ -n "$doc" ] && break
  sleep 0.1
done
[ -n "$doc" ] || { echo "the app never got ready"; cat "$status" 2>/dev/null; exit 1; }
port=$(python3 -c "import json; print(json.load(open('$status'))['port'])")
store=$(python3 -c "import json; print(json.load(open('$status'))['store'])")
case "$store" in "$data"/*) ;; *) echo "the app's store is outside its container: $store"; exit 1 ;; esac
echo "the iPad app in the simulator on the text engine with array beside it: control port $port, document $doc, store in its container"
set +e
timeout 600 "$runner" --port "$port" --token-file "$t/token" --doc "$doc" --measured --engine text --engine array \
  --cases "$root/crates/editor-core/fixtures/conformance/cases.json" --half "$half"
code=$?
set -e
xcrun simctl terminate "$sim" "$bundle" && echo "the app was terminated"
exit $code
