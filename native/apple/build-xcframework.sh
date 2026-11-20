#!/bin/bash
# Build plumcraft-ffi into native/apple/PlumCraftFFI/PlumCraftFFI.xcframework (not committed),
# as the native virtimp.com app's LocalRenderer/build-xcframework.sh does.
#
#   native/apple/build-xcframework.sh                       # macOS (arm64 + x86_64), iOS, iOS simulator (arm64)
#   PLATFORMS=macos native/apple/build-xcframework.sh        # just the Mac
#   MAC_TARGETS=aarch64-apple-darwin PLATFORMS=macos ...     # just this Mac's architecture
#
# Least disk: release without debug info, into the workspace's own target/ (one directory per
# target triple, which cargo makes anyway), no other target directory. The static library holds
# the session and both in-process engines (text and array; the Swift kit's tests open a core on
# each); the xcframework's headers are crates/ffi/include. The Swift kit that uses it is the
# package beside this script (native/apple/PlumCraftFFI: swift build && swift test).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target_dir="$root/target"
out="$here/PlumCraftFFI/PlumCraftFFI.xcframework"
headers="$root/crates/ffi/include"
platforms="${PLATFORMS:-macos ios ios-sim}"
mac_targets="${MAC_TARGETS:-aarch64-apple-darwin x86_64-apple-darwin}"
export CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_STRIP=debuginfo
# the shells' oldest systems (research note 24, decision 12), or the linker warns on every object
export MACOSX_DEPLOYMENT_TARGET=15.0 IPHONEOS_DEPLOYMENT_TARGET=18.0

build() { (cd "$root" && cargo build --release -p plumcraft-ffi --lib --target "$1"); }
lib() { echo "$target_dir/$1/release/libplumcraft_ffi.a"; }

args=()
for p in $platforms; do
  case "$p" in
    macos)
      libs=()
      for t in $mac_targets; do build "$t"; libs+=("$(lib "$t")"); done
      mkdir -p "$target_dir/universal-apple-darwin/release"
      lipo -create "${libs[@]}" -output "$target_dir/universal-apple-darwin/release/libplumcraft_ffi.a"
      args+=(-library "$target_dir/universal-apple-darwin/release/libplumcraft_ffi.a" -headers "$headers") ;;
    ios) build aarch64-apple-ios; args+=(-library "$(lib aarch64-apple-ios)" -headers "$headers") ;;
    ios-sim) build aarch64-apple-ios-sim; args+=(-library "$(lib aarch64-apple-ios-sim)" -headers "$headers") ;;
    *) echo "unknown platform $p" >&2; exit 2 ;;
  esac
done

rm -rf "$out"
xcodebuild -create-xcframework "${args[@]}" -output "$out" >/dev/null
echo "built $out ($platforms)"
for a in "${args[@]}"; do case "$a" in *.a) echo "  $(du -h "$a" | cut -f1)  $a" ;; esac; done
