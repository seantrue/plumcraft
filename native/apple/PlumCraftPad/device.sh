#!/bin/bash
# The iPad app on a physical iPad, in one command, once it is connected, unlocked and on the same
# network as this Mac (N4 slice 6, from slice 4's script; the N4 device gate):
#   DEVICE=<udid> TEAM=<team id> native/apple/PlumCraftPad/device.sh [conformance|ui|all]   (default all)
# OUT names the folder for logs and copies (default: a new one under $TMPDIR).
#
# conformance: this branch's DEBUG build of com.plumcraft.pad (signed automatically with the
#   team's existing identity and profile; no -allowProvisioningUpdates, nothing asked of Apple),
#   installed over the one there, launched with its control port on the iPad's local-network
#   address (--control-bind lan) with a fresh pre-shared token from the launch arguments, on an
#   empty scratch graph (--open-empty); the whole suite runs from this Mac over the network
#   (plumcraft-conformance --host IP); then the app is quit by its pid. iOS may ask once whether
#   the debug build may use the local network (its Info.plist says why): a person answers that on
#   the iPad.
# ui:     the iPad UI tests on the device. The tests read the app's status from the debug build's
#   status element (accessibility), since the test runner's sandbox can't see the app's files on a
#   device; each run has a result bundle of its own.
# Nothing here asks Apple for anything: signing uses the team's existing identity and profile.
# Exit 5: the iPad was locked (a launch refused); nothing after it was read.
set -uo pipefail

# what the iPad needs, said before anything is done (N4 slice 6; the device gate added the last three)
cat <<'NEEDS'
Before this runs, the iPad needs:
  - connected to this Mac (cable or the same network) and UNLOCKED, and kept unlocked:
    Settings > Display & Brightness > Auto-Lock set to Never for the run, and on power;
  - on the same local network as this Mac, with any VPN OFF for the suite (the app listens on
    the iPad's own address; a VPN's tunnel address can't be reached from here);
  - for the budget runs: Low Power Mode OFF, and Settings > Accessibility > Motion >
    Limit Frame Rate OFF (either caps the display at 60 Hz);
  - for the UI tests: Settings > Developer > Enable UI Automation ON (without it the tests stop
    at once with "Authentication canceled");
  - to answer once, on the iPad, iOS's question whether the debug build may use the local network.
NEEDS
what="${1:-all}"
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
pad="$here"
kit="$root/native/apple/PlumCraftFFI"
team="${TEAM:?set TEAM to the signing team id}"
dev="${DEVICE:-$(xcrun devicectl list devices 2>/dev/null | grep -E 'iPad.*physical' | grep -E 'available|connected' | head -1 | grep -oE '[0-9A-F]{8}-[0-9A-F]{16}')}"
[ -n "$dev" ] || { echo "no iPad connected: set DEVICE to its UDID (xcrun devicectl list devices)"; exit 2; }
bundle=com.plumcraft.pad
port="${CONTROL_PORT:-47811}"
out="${OUT:-$(mktemp -d "${TMPDIR:-/tmp}/plumcraft-device.XXXXXX")}"; mkdir -p "$out"; echo "logs in $out"
check="$here/subnet-check.sh"
run_id="$(date +%H%M%S)-$$"
# a launch's output said the iPad refused it because it is locked
locked() { grep -qE 'BSErrorCodeDescription = Locked|could not be, unlocked|device is locked|Locked \(' "$1"; }
available() { timeout 60 xcrun devicectl list devices 2>/dev/null | grep "$dev" | grep -qE 'available|connected'; }
available || { echo "the iPad $dev isn't available (not connected, or locked): nothing done"; exit 2; }
pid=""
cleanup() {
  # quit exactly the app this script launched, by its pid; never another app
  if [ -n "$pid" ]; then timeout 60 xcrun devicectl device process signal --device "$dev" --pid "$pid" --signal SIGTERM >/dev/null 2>&1 || true; fi
}
trap cleanup EXIT INT TERM

conformance() {
  cd "$pad" && command -v xcodegen >/dev/null && xcodegen generate --quiet
  timeout 1200 xcodebuild -project PlumCraftPad.xcodeproj -target PlumCraftPad -configuration Debug -sdk iphoneos \
    SYMROOT="$pad/build-device" OBJROOT="$pad/build-device/obj" \
    CODE_SIGN_STYLE=Automatic DEVELOPMENT_TEAM="$team" CODE_SIGN_IDENTITY="Apple Development" build >"$out/build.log" 2>&1 \
    || { grep -E 'error:' "$out/build.log" | sort -u; echo "the device build failed (signing?): stopped"; return 1; }
  app="$pad/build-device/Debug-iphoneos/PlumCraftPad.app"
  /usr/libexec/PlistBuddy -c "Print :NSLocalNetworkUsageDescription" "$app/Info.plist" || { echo "the debug build lacks its local-network text"; return 1; }
  (cd "$kit" && timeout 900 swift build -c release --product plumcraft-conformance -q) || return 1
  timeout 300 xcrun devicectl device install app --device "$dev" "$app" >"$out/install.log" 2>&1 || { cat "$out/install.log"; return 1; }
  token="$(openssl rand -hex 32)"
  printf '%s\n' "$token" > "$out/token"; chmod 600 "$out/token"
  # a status file of this run's own: one an earlier run left in the app's tmp is never read
  status="status-$run_id.json"
  rm -f "$out/launch.json" "$out/status.json"
  timeout 120 xcrun devicectl device process launch --device "$dev" --terminate-existing --json-output "$out/launch.json" "$bundle" \
    --status-file "$status" --scratch-state --engine text --open-empty \
    --control-bind lan --control-port "$port" --control-token "$token" >"$out/launch.log" 2>&1 || {
      if locked "$out/launch.log"; then echo "the iPad is locked: the launch was refused (unlock it; Auto-Lock to Never)"; return 5; fi
      cat "$out/launch.log"; return 1; }
  pid="$(python3 -c "import json;d=json.load(open('$out/launch.json'));print(d['result']['process']['processIdentifier'])" 2>/dev/null)"
  echo "launched pid $pid at $(date '+%H:%M:%S')"
  # the status file, copied back from the app's container: the address and port it listens on
  ip=""; doc=""
  for _ in $(seq 60); do
    timeout 60 xcrun devicectl device copy from --device "$dev" --domain-type appDataContainer --domain-identifier "$bundle" \
      --source "tmp/$status" --destination "$out/status.json" >/dev/null 2>&1
    read -r ip doc < <(python3 -c "
import json
s=json.load(open('$out/status.json')); w=s.get('windows',[]); c=s.get('control',{})
print(c.get('bind',''), w[0]['doc'] if w and w[0].get('ready') else '')" 2>/dev/null)
    [ -n "$ip" ] && [ -n "$doc" ] && break
    if [ -n "$ip" ] && ! "$check" "$ip" >/dev/null; then break; fi
    sleep 2
  done
  [ -n "$ip" ] || { echo "the app never said where it listens"; cat "$out/status.json" 2>/dev/null; return 1; }
  # an address this Mac can't reach (a VPN's tunnel on the iPad) fails at once, not after a wait
  "$check" "$ip" || { echo "the app listens on $ip, which this Mac can't reach: turn the iPad's VPN off and run this again"; return 1; }
  [ -n "$doc" ] || { echo "the app listens on $ip but its graph never got ready"; cat "$out/status.json" 2>/dev/null; return 1; }
  echo "the app on the iPad listens on $ip:$port (document $doc)"
  timeout 900 "$kit/.build/release/plumcraft-conformance" --host "$ip" --port "$port" --token-file "$out/token" --doc "$doc" \
    --measured --engine text --engine array --cases "$root/crates/editor-core/fixtures/conformance/cases.json" --half all | tee "$out/conformance.log"
  code=${PIPESTATUS[0]}
  timeout 60 xcrun devicectl device info files --device "$dev" --domain-type appDataContainer --domain-identifier "$bundle" >/dev/null 2>&1 || true
  cleanup; pid=""
  echo "quit at $(date '+%H:%M:%S')"
  return "$code"
}

ui() {
  cd "$pad"
  # a result bundle of this run's own (xcodebuild refuses to write over one), never an old one read
  local result="$out/ui-$run_id.xcresult"
  rm -rf "$result"
  TEST_RUNNER_PLUMCRAFT_SHOTS="$out" TEST_RUNNER_PLUMCRAFT_STATUS_VIA=element timeout 1800 xcodebuild -project PlumCraftPad.xcodeproj -scheme PlumCraftPad -configuration Debug \
    -derivedDataPath "$pad/build-device/test" -destination "id=$dev" CODE_SIGN_STYLE=Automatic DEVELOPMENT_TEAM="$team" \
    -resultBundlePath "$result" test >"$out/ui-full-$run_id.log" 2>&1
  local code=$?
  grep -E 'Test Case|error:|TEST (SUCCEEDED|FAILED)' "$out/ui-full-$run_id.log" | tee "$out/ui-$run_id.log"
  if locked "$out/ui-full-$run_id.log"; then echo "the iPad is locked: the UI tests didn't run (unlock it; Auto-Lock to Never)"; code=5; fi
  if grep -q 'Authentication canceled' "$out/ui-full-$run_id.log"; then
    echo "UI automation was refused: Settings > Developer > Enable UI Automation on the iPad, then run again"; code=6
  fi
  [ -d "$result" ] && xcrun xcresulttool export attachments --path "$result" --output-path "$out" >/dev/null 2>&1 || true
  timeout 60 xcrun devicectl device uninstall app --device "$dev" com.plumcraft.pad.uitests.xctrunner >/dev/null 2>&1 || true
  return "$code"
}

case "$what" in
  conformance) conformance; exit $? ;;
  ui) ui; exit $? ;;
  all) conformance; c=$?; [ "$c" = 5 ] && exit 5; ui; u=$?; [ "$c" = 0 ] && c=$u; exit $c ;;
  *) echo "usage: $0 [conformance|ui|all]"; exit 2 ;;
esac
