#!/bin/sh
# Private strings gate: no host names, device ids, home paths, private project names or local
# network addresses in tracked files (this repository is public).
#
#   scripts/check-private-strings.sh [PATTERN_FILE]
#
# The patterns are not in this script, since it is tracked. They are read from PATTERN_FILE,
# by default scripts/private-strings.local (ignored by git): one extended regex per line, matched
# case-insensitively against every tracked text file. Blank lines and lines starting with `#` are
# skipped. A line `skip PATH` leaves out one tracked file (for a match that is functional and
# kept on purpose; say why in a comment above it). scripts/private-strings.local.example holds
# the generic shapes to start from.
#
# Exits 0 when nothing matches, 1 on a match (each one is printed as file:line: text), 2 when the
# pattern file is missing or holds no patterns. Not run in CI: CI has no pattern file.
set -u
cd "$(dirname "$0")/.."
pats=${1:-scripts/private-strings.local}
if [ ! -r "$pats" ]; then
  echo "check-private-strings: no pattern file at $pats" >&2
  echo "  copy scripts/private-strings.local.example to scripts/private-strings.local and add the real strings" >&2
  exit 2
fi
tmp=$(mktemp -d "${TMPDIR:-/tmp}/private-strings.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT
grep -v '^[[:space:]]*#' "$pats" | grep -v '^[[:space:]]*$' | grep -v '^skip ' >"$tmp/regex"
grep '^skip ' "$pats" | sed 's/^skip //' >"$tmp/skip"
if [ ! -s "$tmp/regex" ]; then
  echo "check-private-strings: $pats holds no patterns" >&2
  exit 2
fi
# this script and the example hold shapes, not strings
printf '%s\n' scripts/check-private-strings.sh scripts/private-strings.local.example >>"$tmp/skip"
git ls-files | grep -vxF -f "$tmp/skip" >"$tmp/files"
status=0
while IFS= read -r re; do
  hits=$(tr '\n' '\0' <"$tmp/files" | xargs -0 grep -IinE -- "$re" 2>/dev/null)
  if [ -n "$hits" ]; then
    n=$(printf '%s\n' "$hits" | wc -l | tr -d ' ')
    echo "private strings: $n line(s) match pattern $(grep -nxF -- "$re" "$pats" | head -1 | cut -d: -f1) of $pats:"
    printf '%s\n' "$hits" | cut -c1-200 | sed 's/^/  /'
    status=1
  fi
done <"$tmp/regex"
[ "$status" = 0 ] && echo "private strings: no match in $(wc -l <"$tmp/files" | tr -d ' ') tracked files"
exit "$status"
