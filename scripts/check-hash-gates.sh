#!/bin/sh
# The hash gates (the commonkit hash facade, D10): every digest goes through commonkit::digest.
#  1. hash crates appear in the build only under commonkit (normal and build edges);
#  2. exactly one commonkit, 0.2.x, in the runtime (the build scripts' copy, for the build salts
#     (D4), is listed apart under [build-dependencies] and runs only in build scripts);
#  3. no source outside tests, goldens, build scripts and hash-allowlist.txt names an algorithm,
#     `fixed::`/`build::`, or builds a handle from text ("md5:…", "b3:…", f"{algo}:…").
set -u
cd "$(dirname "$0")/.."
status=0
for c in md-5 md5 sha1 sha2 blake3; do
  out=$(cargo tree --locked --workspace --all-features -e normal,build -i "$c" --depth 1 --prefix none 2>/dev/null) || continue
  bad=$(printf '%s\n' "$out" | sed 1d | grep -v '^commonkit v' | grep -v '^$')
  if [ -n "$bad" ]; then echo "hash gate: $c is used outside commonkit: $bad"; status=1; fi
done
tree=$(cargo tree --locked --workspace --all-features -i commonkit -e normal,build --depth 0)
n=$(printf '%s\n' "$tree" | sed '/^\[build-dependencies\]/,$d' | grep -c '^commonkit v0\.2\.')
other=$(printf '%s\n' "$tree" | grep '^commonkit v' | grep -v '^commonkit v0\.2\.')
[ "$n" = 1 ] && [ -z "$other" ] || { echo "hash gate: $n runtime commonkit v0.2 lines, want 1; other versions: ${other:-none}"; status=1; }
rust='Algo::Md5|Algo::B3|digest::fixed|fixed::(raw|nd1|Fixed|Hasher)|digest::build|"md5:|"b3:|md5::|blake3::|sha2::|sha1::'
py='hashlib|import blake3|fixed\.|build\.both|algo=|"md5:|"b3:|f"\{algo\}:'
for f in $(git ls-files '*.rs' | grep -v '/tests/' | grep -v 'build\.rs$' | grep -v '/digest\.rs$' | grep -vxF -f hash-allowlist.txt); do
  # the in-file `#[cfg(test)]` module is a test; doc comments may quote handles
  hits=$(awk '/^#\[cfg\(test\)\]/{exit} !/^[[:space:]]*\/\/[\/!]/{print FILENAME":"FNR": "$0}' "$f" | grep -E "$rust")
  if [ -n "$hits" ]; then echo "$hits"; status=1; fi
done
for f in $(git ls-files 'python/*.py' | grep -v '^python/tests/' | grep -vxF -f hash-allowlist.txt); do
  hits=$(grep -nE "$py" "$f" | grep -vE '^[0-9]+:[[:space:]]*#' | sed "s|^|$f:|")
  if [ -n "$hits" ]; then echo "$hits"; status=1; fi
done
[ $status = 0 ] && echo "hash gates: ok"
exit $status
