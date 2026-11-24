#!/bin/bash
# Which build this is (B151), from an Xcode script phase ("Stamp build", every build): the git
# commit of the source (short), whether tracked files had uncommitted changes, and the time (UTC),
# written into the built app's Info.plist as PlumBuildCommit, PlumBuildDirty ("yes", "no",
# "unknown") and PlumBuildDate before the app is signed. The app hands them to the core
# (`app.built`), whose About line shows them. With no git, or outside a repository, they read
# "unknown" and the build goes on. Nothing personal is written: no path, no name.
set -u
plist="${TARGET_BUILD_DIR:-}/${INFOPLIST_PATH:-}"
[ -f "$plist" ] || { echo "stamp-build: no Info.plist at $plist; not stamped"; exit 0; }
src="${SRCROOT:-.}"
commit=$(git -C "$src" rev-parse --short=7 HEAD 2>/dev/null) || commit=unknown
[ -n "$commit" ] || commit=unknown
dirty=unknown
if [ "$commit" != unknown ]; then
  if changes=$(git --no-optional-locks -C "$src" status --porcelain --untracked-files=no 2>/dev/null); then
    if [ -n "$changes" ]; then dirty=yes; else dirty=no; fi
  fi
fi
if [ -n "${SOURCE_DATE_EPOCH:-}" ]; then
  date=$(date -u -r "$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null) || date=unknown
else
  date=$(date -u +%Y-%m-%dT%H:%M:%SZ)
fi
pb=/usr/libexec/PlistBuddy
for kv in "PlumBuildCommit:$commit" "PlumBuildDirty:$dirty" "PlumBuildDate:$date"; do
  k="${kv%%:*}"; v="${kv#*:}"
  "$pb" -c "Delete :$k" "$plist" >/dev/null 2>&1
  "$pb" -c "Add :$k string $v" "$plist" || { echo "stamp-build: could not write $k"; exit 0; }
done
echo "stamp-build: $commit dirty=$dirty $date"
