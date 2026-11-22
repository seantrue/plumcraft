#!/bin/bash
# The person check (decisions after B70 to B73: "Add it to every shell's gate"): launch the built
# app the way the Finder does and use it the way a person does. B70 (File › New did nothing), B71
# (Install Engine… disabled, no installed engine offered) and B73 (forms laid out wrongly) were each
# reported as working, because every test launched the app with a file to open, `--engine`,
# `--no-open-panel` and the control port, and nobody looked at what a person sees.
#
#   native/apple/PlumCraftMac/person-check.sh                 build (Release) and check
#   APP=path/Plummer.app native/apple/PlumCraftMac/person-check.sh    that app as it is
#   SHOTS=DIR …                                               keep the sheets' pictures in DIR
#
# The launch: the app's program itself (never `open`, which could hand the request to a copy the
# person has running), in a bare environment as launchd gives a Finder launch (HOME, USER, LOGNAME,
# SHELL, TMPDIR and PATH=/usr/bin:/bin:/usr/sbin:/sbin; no shell PATH), with no file, no
# `--engine` and no `--no-open-panel`. What keeps it off the person's data: `--scratch-state` and
# the scratch environment (store, marks, caches, log, preferences, craft root, test run, and the
# engines folder, which holds one engine of the check's own). The driving and the looking:
# `--status-file` and `--control-token-file` (read only), and `--then inbox:DIR`, the app's own
# steps (PersonSteps.swift): menu items of the real menu bar picked, sheets filled and clicked, the
# palette and the canvas's drags. What `--scratch-state` changes that a person would see is in
# docs/research/24-native-shells.md (the person check).
#
# Each step prints `PERSON PASS|FAIL|SEE | step | what it saw`. The app is never activated and
# never takes the keyboard; it is quit from its menu, then by its PID if it is still there. It runs
# as a copy of the build under the scripted bundle id (scripted-app.sh, B84), so its Save and Open
# panels and Open Recent are used as a person uses them, on scratch folders only; the person's
# Library for both their ids, their home folder's and ~/workspace's top level, ~/Documents and
# ~/Desktop are compared before and after (library-check.sh), as is ~/.config/plumcraft/engines,
# and the scripted id's Library is removed at the end. PROOF=DIR keeps the before/after listings.
# Local runs only (no macOS CI). Exits non-zero when a step failed.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/plummer-person.XXXXXX")"
scratch="$(cd "$scratch" && pwd -P)"
shots="${SHOTS:-$scratch/shots}"
pid=""
copy=""
cleanup() {
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then kill -TERM "$pid" 2>/dev/null; sleep 2; kill -9 "$pid" 2>/dev/null || true; fi
  # the scripted copy and what the system kept for its id go with the run (B84)
  [ -n "$copy" ] && "$here/scripted-app.sh" clean "$copy"
  [ -n "${KEEP:-}" ] || rm -rf "$scratch"
}
trap cleanup EXIT INT TERM

cd "$here"
if [ -n "${APP:-}" ]; then
  bundle="$APP"
else
  command -v xcodegen >/dev/null && xcodegen generate --quiet
  derived="${DERIVED:-build}"
  timeout 900 xcodebuild -project PlumCraftMac.xcodeproj -scheme PlumCraftMac -configuration Release \
    -derivedDataPath "$derived" -destination 'platform=macOS,arch=arm64' build -quiet 2>&1 | grep -v 'DVTProvisioning\|IDERunDestination' || true
  case "$derived" in /*) ;; *) derived="$here/$derived" ;; esac
  bundle="$derived/Build/Products/Release/Plummer.app"
fi
[ -x "$bundle/Contents/MacOS/Plummer" ] || { echo "no app at $bundle"; exit 2; }
person_id=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$bundle/Contents/Info.plist")
# B84 (decision 7 after the person check): the check runs a copy of the build under the scripted
# bundle id, so the Save and Open panels' folders, Open Recent's list and saved state are the
# copy's; the person's ids (this app's and the egui editor's) must be left exactly as they were
scripted_id="$person_id.scripted"
person_ids="$person_id com.swapwizard.plumcraft"
"$here/scripted-app.sh" clean "$scripted_id"
copy=$("$here/scripted-app.sh" make "$bundle" "$scratch/app") || { echo "could not make the scripted copy"; exit 2; }
app="$copy/Contents/MacOS/Plummer"
bundle_id=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$copy/Contents/Info.plist")
[ "$bundle_id" = "$scripted_id" ] || { echo "the copy's id is $bundle_id, not $scripted_id"; exit 2; }

# the eyes from outside (accessibility), built once beside the app's build
ax="$here/build/person-check-ax"
if [ ! -x "$ax" ] || [ "$here/person-check-ax.swift" -nt "$ax" ]; then
  mkdir -p "$here/build"
  timeout 300 swiftc -O "$here/person-check-ax.swift" -o "$ax"
fi

# an engine installed in the scratch engines folder, started by the app as a process of its own:
# the text engine's program (the one inside the app when it carries engines, else the workspace's
# release build), copied into scratch
engine_src="${ENGINE_BIN:-}"
[ -n "$engine_src" ] || { [ -x "$bundle/Contents/MacOS/plumcraft-text-engine" ] && engine_src="$bundle/Contents/MacOS/plumcraft-text-engine"; } || true
[ -n "$engine_src" ] || engine_src="${CARGO_TARGET_DIR:-$root/target}/release/plumcraft-text-engine"
[ -x "$engine_src" ] || { echo "no text engine program at $engine_src (cargo build --release -p plumcraft-text)"; exit 2; }
mkdir -p "$scratch/engines" "$scratch/bin" "$scratch/inbox" "$scratch/store" "$scratch/marks" "$scratch/run" "$scratch/craft"
cp "$engine_src" "$scratch/bin/person-text-engine"
label="Person Check Text"
cat >"$scratch/engines/person.toml" <<EOF
# the person check's installed engine: the text engine's program, as a process of the app's
id = "person_text"
name = "$label"
mode = "process"
command = ["$scratch/bin/person-text-engine"]
restart = "on-crash"
max_restarts = 2
EOF
# decision 17: a copy of a project's manifest away from its project, which warns (its folder has
# no pyproject.toml), marked in the Engine menu and, once in use, in the engine cell. So that it
# can be in use, its stand-in uv starts the text engine's program instead of failing as a real
# copy's would (the warning is judged from the manifest alone)
cat >"$scratch/engines/copied.toml" <<'EOF'
id = "copied"
name = "Copied Engine"
mode = "process"
command = ["./uv", "run", "--project", ".", "python", "-m", "copied_engine"]
cwd = "."
EOF
printf '#!/bin/sh\nexec "%s"\n' "$scratch/bin/person-text-engine" >"$scratch/engines/uv"
chmod 755 "$scratch/engines/uv"
# B87: a manifest the core can't read (its id has a hyphen), which must be said, not skipped
cat >"$scratch/engines/broken.toml" <<EOF
id = "person-broken"
mode = "process"
command = ["$scratch/bin/person-text-engine"]
EOF

# before: the person's Library (both ids), the person's folders, the engines folder, and the
# scripted id's Library (empty: cleaned above)
"$here/library-check.sh" snapshot "$person_ids" "$scratch/library-before.txt"
"$here/library-check.sh" home "$scratch/home-before.txt"
"$here/library-check.sh" snapshot "$scripted_id" "$scratch/scripted-before.txt"
person_engines="$HOME/.config/plumcraft/engines"
snap_engines() { if [ -d "$person_engines" ]; then (cd "$person_engines" && ls -la && shasum -a 256 ./* 2>/dev/null); else echo absent; fi; }
snap_engines >"$scratch/engines-before.txt"

# decision 7: a scripted launch refuses without a scratch engines folder (unset, or outside scratch)
code=0
# the refusal in front of any Save or Open press in a real panel (none is pressed: the press
# needs PERSON_PRESS_SAVE=1, which no script sets, and both panel fields verified scratch ones)
[ -z "${PERSON_PRESS_SAVE:-}" ] || { echo "PERSON FAIL | PERSON_PRESS_SAVE is set: the person check does not press Save in a real panel; unset it"; exit 2; }
python3 "$here/person-check.py" --self-test || { echo "PERSON FAIL | the refusal before a Save press is broken: nothing launched"; exit 2; }
refused() {
  local why
  why=$(env -i HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    PLUMCRAFT_EDITOR_PREFS="$scratch/prefs.json" COMMONKIT_STORE_ROOT="$scratch/store" COMMONKIT_MARK_DIR="$scratch/marks" \
    COMMONKIT_TEST_RUN="$scratch/run" PLUMCRAFT_CRAFT_ROOT="$scratch/craft" "$@" \
    "$(command -v timeout)" 60 "$app" --scratch-state --status-file "$scratch/refused.json" 2>&1 >/dev/null; echo "exit $?")
  case "$why" in
    *PLUMCRAFT_ENGINE_FOLDER*"exit 3") echo "PERSON PASS | $1: the scripted launch refuses | ${why//$'\n'/ }" ;;
    *) echo "PERSON FAIL | $1: the scripted launch should refuse | ${why//$'\n'/ }"; code=1 ;;
  esac
}
refused "PLUMCRAFT_ENGINE_FOLDER_UNSET=1"
refused "PLUMCRAFT_ENGINE_FOLDER=$HOME/.config/plumcraft/engines"
# B84: the build under the person's own id refuses a scripted launch whatever its environment
why=$(env -i HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  PLUMCRAFT_EDITOR_PREFS="$scratch/prefs.json" COMMONKIT_STORE_ROOT="$scratch/store" COMMONKIT_MARK_DIR="$scratch/marks" \
  COMMONKIT_TEST_RUN="$scratch/run" PLUMCRAFT_CRAFT_ROOT="$scratch/craft" PLUMCRAFT_ENGINE_FOLDER="$scratch/engines" \
  "$(command -v timeout)" 60 "$bundle/Contents/MacOS/Plummer" --scratch-state --status-file "$scratch/refused.json" 2>&1 >/dev/null; echo "exit $?")
case "$why" in
  *"$scripted_id"*"exit 3") echo "PERSON PASS | the person's build ($person_id) refuses a scripted launch | ${why//$'\n'/ }" ;;
  *) echo "PERSON FAIL | the person's build ($person_id) should refuse a scripted launch | ${why//$'\n'/ }"; code=1 ;;
esac

# the launch, as the Finder's (a bare environment), on scratch
status="$scratch/status.json"
env -i HOME="$HOME" USER="${USER:-}" LOGNAME="${LOGNAME:-${USER:-}}" SHELL="${SHELL:-/bin/zsh}" TMPDIR="${TMPDIR:-/tmp}" \
  PATH=/usr/bin:/bin:/usr/sbin:/sbin __CF_USER_TEXT_ENCODING="${__CF_USER_TEXT_ENCODING:-0x0:0:0}" \
  PLUMCRAFT_EDITOR_PREFS="$scratch/prefs.json" COMMONKIT_STORE_ROOT="$scratch/store" COMMONKIT_MARK_DIR="$scratch/marks" \
  COMMONKIT_TEST_RUN="$scratch/run" PLUMCRAFT_CRAFT_ROOT="$scratch/craft" COMMONKIT_STORE_MIN_FREE_GB=2 \
  PLUMCRAFT_ENGINE_FOLDER="$scratch/engines" \
  "$app" --scratch-state --status-file "$status" --control-token-file "$scratch/token" --then "inbox:$scratch/inbox" \
  >"$scratch/app.log" 2>&1 &
pid=$!
echo "PERSON launched $app (pid $pid) with nothing to open, in a bare environment, on scratch $scratch"

set +e
timeout 600 python3 "$here/person-check.py" --pid "$pid" --status "$status" --inbox "$scratch/inbox" --token "$scratch/token" \
  --ax "$ax" --scratch "$scratch" --shots "$shots" --engine-label "$label" --engine-bin "$scratch/bin/person-text-engine"
py=$?
[ "$py" -eq 0 ] || code=1
set -e

# quit: the menu's Quit was picked last; the app must go by itself, else TERM, else KILL (each by PID)
gone=""
for _ in $(seq 150); do kill -0 "$pid" 2>/dev/null || { gone=1; break; }; sleep 0.1; done
if [ -n "$gone" ]; then
  echo "PERSON PASS | Plummer › Quit Plummer | the app quit by itself"
else
  echo "PERSON FAIL | Plummer › Quit Plummer | still running after 15 s: TERM"
  code=1
  kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 100); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
  kill -0 "$pid" 2>/dev/null && { echo "the app did not quit on TERM; killed"; kill -9 "$pid"; }
fi
wait "$pid" 2>/dev/null || true
pid=""
grep -i 'refuses\|error\|panic' "$scratch/app.log" | head -20 || true
# the engine the app started went with it
left=$(pgrep -f "[p]erson-text-engine" 2>/dev/null | while read -r p; do ps -o command= -p "$p" | grep -q "$scratch" && echo "$p"; done || true)
[ -z "$left" ] || { echo "PERSON FAIL | the installed engine's process outlived the app | pids $left"; code=1; kill -9 $left 2>/dev/null || true; }

sleep "${SETTLE:-10}"
"$here/library-check.sh" snapshot "$person_ids" "$scratch/library-after.txt"
"$here/library-check.sh" compare "$scratch/library-before.txt" "$scratch/library-after.txt" || code=1
"$here/library-check.sh" home "$scratch/home-after.txt"
"$here/library-check.sh" home-compare "$scratch/home-before.txt" "$scratch/home-after.txt" || code=1
# what the system kept for the scripted copy (the panels' folder, Open Recent's list), then gone
"$here/library-check.sh" snapshot "$scripted_id" "$scratch/scripted-after.txt"
echo "PERSON what the system kept for $scripted_id during the run:"
diff "$scratch/scripted-before.txt" "$scratch/scripted-after.txt" | grep '^>' | cut -c1-220 || true
"$here/scripted-app.sh" clean "$copy"; copy=""
"$here/library-check.sh" snapshot "$scripted_id" "$scratch/scripted-cleaned.txt"
if diff -q "$scratch/scripted-before.txt" "$scratch/scripted-cleaned.txt" >/dev/null; then
  echo "PERSON PASS | the scripted copy's Library, cleaned | as before the run (nothing kept for $scripted_id)"
else
  echo "PERSON FAIL | the scripted copy's Library, cleaned | something is left:"; diff "$scratch/scripted-before.txt" "$scratch/scripted-cleaned.txt"; code=1
fi
[ -z "${PROOF:-}" ] || cp "$scratch"/library-*.txt "$scratch"/home-*.txt "$scratch"/scripted-*.txt "$PROOF/" 2>/dev/null || true
snap_engines >"$scratch/engines-after.txt"
if diff -u "$scratch/engines-before.txt" "$scratch/engines-after.txt"; then
  echo "PERSON PASS | the person's engines folder | unchanged"
else
  echo "PERSON FAIL | the person's engines folder | changed (above)"
  code=1
fi
echo "PERSON pictures in $shots"
exit $code
