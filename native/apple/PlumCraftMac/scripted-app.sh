#!/bin/bash
# B84, decision 7 after the person check ("scripted launches get their own bundle id"): a scripted
# launch (`--scratch-state`) of the Mac app runs as a copy of the built app under a bundle id of
# its own, the person's id plus `.scripted`, so whatever the system keeps for an app by its id
# (the defaults domain AppKit's Save and Open panels write their folder to, the recent documents
# behind File › Open Recent, saved window state, caches) is that copy's, never the person's. The
# app refuses `--scratch-state` under any other id (ScratchGuard.bundleRefusal).
#
#   scripted-app.sh make SRC.app DIR    a copy of SRC.app in DIR (a clone: no extra disk on APFS),
#                                       its id the scripted one, signed again for this Mac only
#                                       (ad hoc, nothing sent anywhere); prints the copy's path
#   scripted-app.sh clean DIR/X.app     what the system keeps for the copy's id, removed (the
#                                       defaults domain, saved state, recent documents, caches,
#                                       HTTP storage, WebKit, the temporary folder's state), and
#                                       the copy unregistered from Launch Services (so the Finder
#                                       never offers it for a file) and removed. Refuses any id
#                                       that is not a scripted one.
#   scripted-app.sh locations ID        the Library locations kept for ID, one a line
#
# The script that makes a copy cleans it when it ends (conformance.sh, person-check.sh, bench.sh),
# and cleans the scripted id before it starts too, in case an earlier run was killed.
set -uo pipefail
L="$HOME/Library"
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

locations() {
  local id="$1"
  echo "$L/Preferences/$id.plist"
  find "$L/Preferences/ByHost" -maxdepth 1 -name "$id.*" 2>/dev/null | sort
  echo "$L/Saved Application State/$id.savedState"
  for f in "$L/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments/$id".sfl*; do
    [ -e "$f" ] && echo "$f"
  done
  echo "$L/Caches/$id"
  echo "$L/HTTPStorages/$id"
  echo "$L/HTTPStorages/$id.binarycookies"
  echo "$L/WebKit/$id"
  echo "$L/Cookies/$id.binarycookies"
  echo "$L/Application Support/$id"
  echo "$(getconf DARWIN_USER_TEMP_DIR)$id.savedState"
  echo "$(getconf DARWIN_USER_CACHE_DIR)$id"
}

scripted_id() { case "$1" in *.scripted) return 0 ;; *) return 1 ;; esac; }

case "${1:-}" in
make)
  src="${2:?make SRC.app DIR}"; dir="${3:?make SRC.app DIR}"
  [ -d "$src/Contents/MacOS" ] || { echo "scripted-app: no app at $src" >&2; exit 2; }
  id=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$src/Contents/Info.plist") || exit 2
  scripted_id "$id" && { echo "scripted-app: $src is already a scripted copy" >&2; exit 2; }
  mkdir -p "$dir"
  out="$dir/$(basename "$src")"
  rm -rf "$out"
  cp -Rc "$src" "$out" 2>/dev/null || cp -R "$src" "$out" || exit 2
  /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $id.scripted" "$out/Contents/Info.plist" || exit 2
  # the nested engine programs first, then the app (its seal covers them), the hardened runtime
  # and the app's own entitlements kept; ad hoc, for this Mac only
  ent="$(mktemp "${TMPDIR:-/tmp}/scripted-ent.XXXXXX")"
  codesign -d --entitlements :- "$src" >"$ent" 2>/dev/null || true
  [ -s "$ent" ] || printf '<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict/></plist>\n' >"$ent"
  for e in "$out"/Contents/MacOS/*; do
    [ "$(basename "$e")" = "$(basename "$src" .app)" ] && continue
    [ -f "$e" ] && [ -x "$e" ] && { codesign --force --sign - --options runtime --timestamp=none "$e" >/dev/null 2>&1 || { echo "scripted-app: could not sign $e" >&2; rm -f "$ent"; exit 2; }; }
  done
  codesign --force --sign - --options runtime --timestamp=none --entitlements "$ent" "$out" >/dev/null 2>&1 \
    || { echo "scripted-app: could not sign $out" >&2; rm -f "$ent"; exit 2; }
  rm -f "$ent"
  echo "$out" ;;
clean)
  app="${2:?clean X.app}"
  id=""
  [ -f "$app/Contents/Info.plist" ] && id=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$app/Contents/Info.plist" 2>/dev/null)
  # an id given instead of an app (the copy already gone): clean that id's locations
  [ -z "$id" ] && case "$app" in *.app) ;; *) id="$app"; app="" ;; esac
  [ -n "$id" ] || { echo "scripted-app: no id for $app" >&2; exit 2; }
  scripted_id "$id" || { echo "scripted-app: refusing to clean $id: not a scripted id" >&2; exit 2; }
  # cfprefsd holds the domain: ask it to drop the domain before the file goes, or it writes it back
  defaults delete "$id" >/dev/null 2>&1 || true
  locations "$id" | while IFS= read -r p; do
    [ -n "$p" ] || continue
    case "$p" in *"$id"*) ;; *) continue ;; esac
    [ -e "$p" ] && rm -rf "$p"
  done
  if [ -n "$app" ] && [ -d "$app" ]; then
    "$lsregister" -u "$app" >/dev/null 2>&1 || true
    rm -rf "$app"
  fi
  exit 0 ;;
locations) locations "${2:?locations ID}" ;;
*) echo "usage: $0 make SRC.app DIR | clean DIR/X.app | locations ID" >&2; exit 2 ;;
esac
