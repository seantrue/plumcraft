#!/bin/bash
# The conformance suite's read-only half (docs/shell-interface.md 16.1) against the running Mac app,
# which measures its own faces: build the app (release) and the runner, launch the app on scratch
# preferences, store and saved state with its control port open and an empty graph in a window,
# run the cases over the port, quit the app by its PID. Local runs only (no macOS CI).
#
#   native/apple/PlumCraftMac/conformance.sh [read|edit|all]      (default read)
#   APP=path/Plummer.app native/apple/PlumCraftMac/conformance.sh all    (that app, not a fresh build)
#
# Prints a CASE line per case and "N of M cases passed, K skipped"; exits non-zero when one failed.
set -euo pipefail
half="${1:-read}"
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
kit="$here/../PlumCraftFFI"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/plumcraft-mac-conformance.XXXXXX")"
pid=""
# the app goes on every way out (a failure, a time limit, ^C), by its PID
cleanup() {
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then kill -TERM "$pid" 2>/dev/null; sleep 2; kill -9 "$pid" 2>/dev/null || true; fi
  rm -rf "$scratch"
}
trap cleanup EXIT INT TERM
# everything the core and engines write goes to scratch, never the Mac's own store (the login
# shell may name it in COMMONKIT_STORE_ROOT); the app refuses a scripted launch without these
export PLUMCRAFT_EDITOR_PREFS="$scratch/prefs.json" COMMONKIT_HASH=b3 COMMONKIT_STORE_MIN_FREE_GB=2 \
  COMMONKIT_STORE_ROOT="$scratch/store" COMMONKIT_MARK_DIR="$scratch/marks" COMMONKIT_TEST_RUN="$scratch/run" \
  PLUMCRAFT_CRAFT_ROOT="$scratch/craft"

cd "$here"
# APP: an app already built and signed (an exported Developer ID build, say), run as it is
if [ -n "${APP:-}" ]; then
  app="$APP/Contents/MacOS/Plummer"
  [ -x "$app" ] || { echo "no app at $APP"; exit 2; }
else
  command -v xcodegen >/dev/null && xcodegen generate --quiet
  # DERIVED: another build folder (a copy of the app running from build/ is left alone)
  derived="${DERIVED:-build}"
  timeout 900 xcodebuild -project PlumCraftMac.xcodeproj -scheme PlumCraftMac -configuration Release \
    -derivedDataPath "$derived" -destination 'platform=macOS,arch=arm64' build -quiet
  case "$derived" in /*) ;; *) derived="$here/$derived" ;; esac
  app="$derived/Build/Products/Release/Plummer.app/Contents/MacOS/Plummer"
fi
(cd "$kit" && timeout 900 swift build -c release --product plumcraft-conformance -q)
runner="$kit/.build/release/plumcraft-conformance"

run_on() {
status="$scratch/status-$1.json"; rm -f "$status"
"$app" --status-file "$status" --store-root "$scratch/store" --control-token-file "$scratch/token" \
  --no-open-panel --scratch-state --hide-watched --engine "$1" "$scratch/empty.json" >"$scratch/app.log" 2>&1 &
pid=$!
quit() {
  kill -TERM "$pid" 2>/dev/null || return 0
  for _ in $(seq 100); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
  if kill -0 "$pid" 2>/dev/null; then echo "the app did not quit on TERM; killed"; kill -9 "$pid"; return 1; fi
  wait "$pid" && echo "the app quit cleanly" || echo "the app quit with status $?"
  pid=""
}
# wait (at most 60 s) for the port and the window's canvas
doc=""
for _ in $(seq 600); do
  kill -0 "$pid" 2>/dev/null || { echo "the app exited"; cat "$scratch/app.log"; return 1; }
  doc=$(python3 -c "
import json,sys
s=json.load(open('$status')); w=s.get('windows',[])
print(w[0]['doc'] if s.get('port') and w and w[0].get('ready') else '')" 2>/dev/null || true)
  [ -n "$doc" ] && break
  sleep 0.1
done
[ -n "$doc" ] || { echo "the app never got ready"; quit; return 1; }
port=$(python3 -c "import json; print(json.load(open('$status'))['port'])")
echo "the app on the $1 engine, $2 to start when a case uses it: pid $pid, control port $port, document $doc"
set +e
timeout 600 "$runner" --port "$port" --token-file "$scratch/token" --doc "$doc" --measured --engine "$1" --engine "$2" \
  --cases "$root/crates/editor-core/fixtures/conformance/cases.json" --half "$half"
code=$?
set -e
quit
return $code
}

echo '{"version": 1, "nodes": {}, "wires": []}' > "$scratch/empty.json"
failed=0
# every case on one app, as it opens every document: the empty graph on the text engine alone, and
# the picture case's first array node starts the array engine (decision 8C: engines start as the
# graph comes to use them), so no case is skipped for want of an engine
run_on text array || failed=1
exit $failed
