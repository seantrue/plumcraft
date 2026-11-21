#!/bin/bash
# B50's check: a scripted launch (--scratch-state) of the Mac app leaves the person's own Library
# exactly as it was. Reads only; never changes or removes anything it looks at.
#
#   library-check.sh snapshot BUNDLE_ID OUT     write what the Library holds for the app to OUT
#   library-check.sh compare  BEFORE AFTER      exit 1 (and show the difference) when they differ
#
# What it looks at, for BUNDLE_ID: the defaults domain (as cfprefsd answers for it, and its plist's
# size, modification time and hash, ByHost files too), the saved application state (and what
# AppKit leaves in the temporary folder when told to ignore it), the recent documents list, and the
# caches, logs, application support, HTTP storage and WebKit folders the app could write.
# conformance.sh takes a snapshot before the app starts and another after it quits.
#
# Not every writer can be stopped by the app: AppKit's own panels (the colour panel, Open and Save)
# keep their frames and last folder in the app's defaults domain (the old id's domain holds such
# keys), and that domain has no public way to be pointed elsewhere (CFFIXED_USER_HOME and
# __CFPREFERENCES_AVOID_DAEMON both still read the real domain, B50). The conformance cases open
# none of them; a case that does would fail here.
set -uo pipefail
L="$HOME/Library"

# one line per file under each path: path, size, modification time, sha256 (or "absent")
files() {
  local p
  for p in "$@"; do
    if [ ! -e "$p" ]; then echo "absent $p"; continue; fi
    find "$p" \( -type f -o -type l \) -print0 2>/dev/null | sort -z | while IFS= read -r -d '' f; do
      printf '%s %s %s\n' "$f" "$(stat -f '%z %m' "$f")" "$(shasum -a 256 "$f" 2>/dev/null | cut -d' ' -f1)"
    done
    # an empty folder made by the run counts too
    find "$p" -type d -print0 2>/dev/null | sort -z | while IFS= read -r -d '' d; do echo "dir $d $(stat -f '%m' "$d")"; done
  done
}

snapshot() {
  local id="$1" out="$2"
  {
    echo "== defaults $id"
    # cfprefsd's answer (what the app would read back), then the file it keeps
    defaults export "$id" - 2>/dev/null | plutil -p - 2>/dev/null || echo "(no domain)"
    files "$L/Preferences/$id.plist"
    find "$L/Preferences/ByHost" -maxdepth 1 -name "$id.*" 2>/dev/null | sort | while IFS= read -r f; do files "$f"; done
    echo "== saved state"
    files "$L/Saved Application State/$id.savedState"
    # where AppKit keeps the state it is told to ignore (ApplePersistenceIgnoreState): the folder is
    # AppKit's and comes and goes, but nothing may be left in it
    t="$(getconf DARWIN_USER_TEMP_DIR)$id.savedState"
    [ -d "$t" ] && find "$t" -type f 2>/dev/null | sort | while IFS= read -r f; do files "$f"; done
    echo "== recent documents"
    for f in "$L/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments/$id".sfl*; do
      [ -e "$f" ] && files "$f"
    done
    echo "== caches, logs, application support"
    files "$L/Caches/$id" "$L/Logs/Plummer" "$L/Application Support/Plummer" "$L/Application Support/$id" \
      "$L/HTTPStorages/$id" "$L/HTTPStorages/$id.binarycookies" "$L/WebKit/$id" "$L/Cookies/$id.binarycookies"
  } >"$out"
}

case "${1:-}" in
snapshot) snapshot "$2" "$3" ;;
compare)
  if diff -u "$2" "$3"; then
    echo "LIBRARY the person's Library is unchanged"
  else
    echo "LIBRARY FAILED: the scripted run changed the person's Library (above, - before, + after)"
    others=$(pgrep -x Plummer | tr '\n' ' ')
    [ -n "$others" ] && echo "LIBRARY note: other copies of Plummer are running (pids $others); a change may be theirs"
    exit 1
  fi ;;
*) echo "usage: $0 snapshot BUNDLE_ID OUT | compare BEFORE AFTER"; exit 2 ;;
esac
