#!/bin/bash
# Build the native Apple apps (Plummer, since 2026-11-21) for Apple: a Developer ID, hardened, notarization-ready Mac app in a
# DMG, and an App Store .ipa of the iPad and iPhone app for TestFlight. NOTHING here sends anything
# to Apple: no notarization, no upload, no -allowProvisioningUpdates. The commands that do are
# printed at the end, for the maintainer to run (research note 24 section 19, Apple prep).
#
#   native/apple/release.sh mac  [--build N] [--local]
#   native/apple/release.sh ios  [--build N] [--local]
#   native/apple/release.sh both [--build N] [--local]
#
# --build N   the build number (CFBundleVersion), the same for both apps. Default: the UTC time as
#             YYYYMMDDHHMM, which only goes up, as App Store Connect requires.
# --local     check everything without the network: sign with no secure timestamp (the Mac's
#             Developer ID signatures otherwise ask Apple's timestamp server for one), and export the
#             iOS app for development with the profile on this Mac instead of for the App Store.
#             The result is NOT for notarization or upload.
#
# The Mac app carries the engine programs (text, array, audio, craft) in Contents/MacOS, built here
# for MAC_ARCHS; the app and every program in it are signed by hand, nested first, with the
# hardened runtime and no entitlements (PlumCraftMac/PlumCraft.entitlements is empty).
#
# Environment (nothing team-specific is committed):
#   PLUMCRAFT_TEAM_ID               the team id (required)
#   MAC_ARCHS                       default "x86_64 arm64" (both Macs); "arm64" for a quicker check
#   PLUMCRAFT_MAC_IDENTITY          default "Developer ID Application"
#   PLUMCRAFT_IOS_ARCHIVE_IDENTITY  default "Apple Development" (the archive; the export re-signs)
#   PLUMCRAFT_IOS_DIST_IDENTITY     default "Apple Distribution"
#   PLUMCRAFT_IOS_PROFILE           the App Store provisioning profile's name for the iOS bundle id
#                                   (required for the App Store export; not for --local)
#   PLUMCRAFT_NOTARY_PROFILE        the notarytool keychain profile, named in the printed commands
#
# The marketing version is the workspace's (Cargo.toml [workspace.package] version). Everything goes
# to dist/apple/ (not committed). Run native/apple/build-xcframework.sh first.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
what="${1:-}"; shift || true
build="" local=""
while [ $# -gt 0 ]; do
  case "$1" in
    --build) build="${2:?--build needs a number}"; shift ;;
    --local) local=yes ;;
    *) sed -n '2,33p' "$0" >&2; exit 2 ;;
  esac
  shift
done
case "$what" in mac|ios|both) ;; *) sed -n '2,33p' "$0" >&2; exit 2 ;; esac

version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
[ -n "$version" ] || { echo "no workspace version in Cargo.toml" >&2; exit 2; }
build="${build:-$(date -u +%Y%m%d%H%M)}"
[[ $build =~ ^[0-9]+(\.[0-9]+){0,2}$ ]] || { echo "--build $build: one to three dot-separated integers" >&2; exit 2; }
default=$(sed -n 's/^MARKETING_VERSION = //p' "$here/Config/Version.xcconfig")
[ "$default" = "$version" ] || echo "note: Config/Version.xcconfig labels local builds $default; the workspace is $version (this build uses $version)" >&2
team="${PLUMCRAFT_TEAM_ID:?set PLUMCRAFT_TEAM_ID to the signing team id}"
[ -d "$here/PlumCraftFFI/PlumCraftFFI.xcframework" ] || { echo "run native/apple/build-xcframework.sh first" >&2; exit 2; }
out="$root/dist/apple"
mkdir -p "$out"
versions=(MARKETING_VERSION="$version" CURRENT_PROJECT_VERSION="$build")
# both Macs by default (lipo -archs order)
mac_archs="${MAC_ARCHS:-x86_64 arm64}"

# the SHA-1 of the valid signing identity of kind $1 ("Developer ID Application", …) whose
# certificate belongs to the team (its OU; the name's parenthesis is not always the team id),
# checked before anything slow starts
identity_for() {
  local sha name
  while read -r sha name; do
    if security find-certificate -a -Z -c "$name" -p 2>/dev/null | openssl x509 -noout -subject 2>/dev/null | grep -q "OU *= *$team"; then
      echo "$sha"; return 0
    fi
  done < <(security find-identity -v -p codesigning | sed -n "s/^ *[0-9]*) \([0-9A-F]\{40\}\) \"\($1: .*\)\"$/\1 \2/p")
  echo "no valid \"$1\" identity of team $team in the keychain" >&2
  exit 2
}

# an export options plist from a template, with the team and the profile filled in
options() {
  local tpl="$here/Config/$1" dst="$out/$1"
  sed -e "s/@TEAM@/$team/g" -e "s/@PROFILE@/${2:-}/g" -e "s/@BUNDLE@/${3:-}/g" "$tpl" > "$dst"
  plutil -lint "$dst" >/dev/null
  echo "$dst"
}

# the engine programs for the Mac app, one binary per engine with every architecture in MAC_ARCHS
engines() {
  local dir="$1" t triples=() e
  for a in $mac_archs; do case "$a" in arm64) triples+=(aarch64-apple-darwin) ;; x86_64) triples+=(x86_64-apple-darwin) ;; esac; done
  for t in "${triples[@]}"; do
    (cd "$root" && MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_PROFILE_RELEASE_DEBUG=0 cargo build --release --locked --bins \
      -p plumcraft-text -p plumcraft-array -p plumcraft-audio -p plumcraft-craft --target "$t")
  done
  mkdir -p "$dir"
  for e in plumcraft-text-engine plumcraft-array-engine plumcraft-audio-engine plumcraft-craft-engine; do
    local parts=()
    for t in "${triples[@]}"; do parts+=("${CARGO_TARGET_DIR:-$root/target}/$t/release/$e"); done
    lipo -create "${parts[@]}" -output "$dir/$e"
  done
}

# sign nested code first, then the app that seals it (Apple's order; --deep is for checking only)
sign_mac_app() {
  local app="$1" identity="$2" ts="--timestamp" f
  [ -z "$local" ] || ts="--timestamp=none"
  for f in "$app"/Contents/MacOS/plumcraft-*-engine; do
    [ -e "$f" ] && codesign --force --sign "$identity" --options runtime "$ts" "$f"
  done
  codesign --force --sign "$identity" --options runtime "$ts" \
    --entitlements "$here/PlumCraftMac/PlumCraft.entitlements" "$app"
}

mac() {
  local kind="${PLUMCRAFT_MAC_IDENTITY:-Developer ID Application}" identity
  identity=$(identity_for "$kind") || exit 2
  local name="Plummer-$version-$build-macos"
  local archive="$out/$name.xcarchive" export="$out/$name"
  rm -rf "$archive" "$export" "$out/$name.dmg" "$out/$name-engines"
  engines "$out/$name-engines"
  (cd "$here/PlumCraftMac" && xcodegen generate --quiet)
  # the xcframework's library is universal; the engines go in by the Bundle engines phase
  PLUMCRAFT_ENGINES_DIR="$out/$name-engines" timeout 1800 xcodebuild \
    -project "$here/PlumCraftMac/PlumCraftMac.xcodeproj" -scheme PlumCraftMac \
    -configuration Release -destination 'generic/platform=macOS' -archivePath "$archive" \
    ARCHS="$mac_archs" ONLY_ACTIVE_ARCH=NO CODE_SIGN_IDENTITY="$kind" DEVELOPMENT_TEAM="$team" \
    "${versions[@]}" archive -quiet
  # the Developer ID export, by hand: copy the app out of the archive and sign it for distribution
  # (with Apple's secure timestamp, or none with --local), so every nested program is signed the
  # same way, which xcodebuild -exportArchive does not promise for programs in Contents/MacOS
  mkdir -p "$export"
  ditto "$archive/Products/Applications/Plummer.app" "$export/Plummer.app"
  local app="$export/Plummer.app"
  sign_mac_app "$app" "$identity"
  codesign --verify --deep --strict --verbose=2 "$app"
  local f ents info
  for f in "$app"/Contents/MacOS/*; do
    # (captured first: grep -q closing the pipe early fails the pipeline under pipefail)
    ents=$(codesign -d --entitlements - "$f" 2>/dev/null || true)
    info=$(codesign -dv "$f" 2>&1 || true)
    case "$ents" in *get-task-allow*) echo "$f carries get-task-allow: notarization would refuse it" >&2; exit 1 ;; esac
    case "$info" in *"flags=0x"*runtime*) ;; *) echo "$f: no hardened runtime" >&2; exit 1 ;; esac
    [ "$(lipo -archs "$f")" = "$mac_archs" ] || { echo "$f is $(lipo -archs "$f"), not $mac_archs" >&2; exit 1; }
  done
  # the DMG: the app and a link to Applications, signed
  local stage="$out/$name-dmg"
  rm -rf "$stage"; mkdir -p "$stage"
  ditto "$app" "$stage/Plummer.app"
  ln -s /Applications "$stage/Applications"
  hdiutil create -quiet -volname "Plummer $version" -srcfolder "$stage" -format UDZO -ov "$out/$name.dmg"
  rm -rf "$stage"
  if [ -n "$local" ]; then codesign --force --sign "$identity" --timestamp=none "$out/$name.dmg"
  else codesign --force --sign "$identity" --timestamp "$out/$name.dmg"; fi
  codesign --verify --verbose=2 "$out/$name.dmg"
  echo "mac: $out/$name.dmg ($version, build $build; $kind${local:+, local: no timestamp, not for notarization})"
  cat <<EOF

To notarize (sends the DMG to Apple; the maintainer's step, after asking):
  xcrun notarytool submit "$out/$name.dmg" --keychain-profile "${PLUMCRAFT_NOTARY_PROFILE:-plumcraft}" --wait
  xcrun stapler staple "$out/$name.dmg"
  xcrun stapler validate "$out/$name.dmg"
  spctl -a -t open --context context:primary-signature -vv "$out/$name.dmg"
EOF
}

ios() {
  local archive_identity="${PLUMCRAFT_IOS_ARCHIVE_IDENTITY:-Apple Development}"
  local dist_identity="${PLUMCRAFT_IOS_DIST_IDENTITY:-Apple Distribution}"
  identity_for "$archive_identity" >/dev/null || exit 2
  [ -n "$local" ] || identity_for "$dist_identity" >/dev/null || exit 2
  local name="Plummer-$version-$build-ios"
  local archive="$out/$name.xcarchive" export="$out/$name"
  rm -rf "$archive" "$export"
  (cd "$here/PlumCraftPad" && xcodegen generate --quiet)
  local bundle
  bundle=$(cd "$here/PlumCraftPad" && xcodebuild -project PlumCraftPad.xcodeproj -target PlumCraftPad -configuration Release \
    -showBuildSettings 2>/dev/null | sed -n 's/^ *PRODUCT_BUNDLE_IDENTIFIER = //p' | head -1)
  # automatic signing with the profiles already on this Mac (no -allowProvisioningUpdates: nothing
  # is asked of Apple); the export re-signs for the App Store
  timeout 1800 xcodebuild -project "$here/PlumCraftPad/PlumCraftPad.xcodeproj" -scheme PlumCraftPad \
    -configuration Release -destination 'generic/platform=iOS' -archivePath "$archive" \
    CODE_SIGN_STYLE=Automatic CODE_SIGN_IDENTITY="$archive_identity" DEVELOPMENT_TEAM="$team" \
    "${versions[@]}" archive -quiet
  local opts
  if [ -n "$local" ]; then
    opts=$(options ExportOptions-ios-development.plist)
  else
    opts=$(options ExportOptions-ios-app-store.plist "${PLUMCRAFT_IOS_PROFILE:?set PLUMCRAFT_IOS_PROFILE to the App Store profile for $bundle}" "$bundle")
  fi
  timeout 900 xcodebuild -exportArchive -archivePath "$archive" -exportPath "$export" -exportOptionsPlist "$opts"
  local ipa
  ipa=$(ls "$export"/*.ipa)
  echo "ios: $ipa ($version, build $build${local:+; local: a development export, not for TestFlight})"
  cat <<EOF

To upload to App Store Connect for TestFlight (the maintainer's step, after asking), either:
  xcrun altool --upload-app -f "$ipa" -t ios --apiKey KEY_ID --apiIssuer ISSUER_ID
or, from the archive with an upload export:
  xcodebuild -exportArchive -archivePath "$archive" -exportPath "$out/$name-upload" \\
    -exportOptionsPlist <ExportOptions-ios-app-store.plist with destination "upload"> \\
    -authenticationKeyPath ~/.appstoreconnect/private_keys/AuthKey_KEY_ID.p8 \\
    -authenticationKeyID KEY_ID -authenticationKeyIssuerID ISSUER_ID
EOF
}

case "$what" in
  mac) mac ;;
  ios) ios ;;
  both) mac; ios ;;
esac
