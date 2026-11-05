#!/usr/bin/env bash
# Build PlumCraft.app and a DMG for this Mac's architecture, into dist/.
#
#   packaging/macos.sh
#
# CODESIGN_IDENTITY  who signs: a name, or "-" for ad hoc. Default: this keychain's first
#                    "Developer ID Application" identity, else ad hoc.
# NOTARY_PROFILE     a `xcrun notarytool store-credentials` profile. With one, the DMG is notarized
#                    and stapled. Without one, Gatekeeper will warn whoever downloads it.
# SKIP_BUILD=1       use the binaries already in target/release.
# CARGO_BUILD_TARGET build for another Mac: x86_64-apple-darwin on Apple silicon makes an Intel
#                    image (aarch64-apple-darwin the reverse). Needs `rustup target add` once.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

[[ $(uname -s) == Darwin ]] || { echo "macos.sh runs on macOS" >&2; exit 2; }

VERSION=$(version)
case ${CARGO_BUILD_TARGET:-} in
    '') ARCH=$(uname -m) ;;
    x86_64-apple-darwin) ARCH=x86_64 ;;
    aarch64-apple-darwin) ARCH=arm64 ;;
    *) echo "macos.sh builds for x86_64-apple-darwin or aarch64-apple-darwin, not $CARGO_BUILD_TARGET" >&2; exit 2 ;;
esac
APP="$DIST/stage-macos/PlumCraft.app"
DMG="$DIST/PlumCraft-$VERSION-macos-$ARCH.dmg"

identity=${CODESIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application:.*\)"/\1/p' | head -1)}
identity=${identity:--}

[[ -n ${SKIP_BUILD:-} ]] || build_binaries

rm -rf "$DIST/stage-macos" "$DMG"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
copy_binaries "$APP/Contents/MacOS"
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/Info.plist.in" >"$APP/Contents/Info.plist"
cp "$ROOT/assets/app-icon/PlumCraft.icns" "$APP/Contents/Resources/PlumCraft.icns"
write_licences "$APP/Contents/Resources/licences"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

sign() {
    local options=(--force --sign "$identity")
    if [[ $identity != - ]]; then
        options+=(--options runtime --timestamp)
    fi
    codesign "${options[@]}" "$@"
}

# Inner binaries first: signing the bundle seals them.
for name in "${BINARIES[@]}"; do
    [[ $name == plumcraft-editor ]] || sign "$APP/Contents/MacOS/$name"
done
sign "$APP"
codesign --verify --deep --strict "$APP"

mkdir -p "$DIST/stage-macos/image"
mv "$APP" "$DIST/stage-macos/image/"
ln -s /Applications "$DIST/stage-macos/image/Applications"
hdiutil create -quiet -volname "PlumCraft $VERSION" -srcfolder "$DIST/stage-macos/image" -format UDZO -ov "$DMG"

if [[ $identity != - ]]; then
    sign "$DMG"
fi
if [[ -n ${NOTARY_PROFILE:-} ]]; then
    [[ $identity != - ]] || { echo "notarizing needs a Developer ID signature" >&2; exit 2; }
    xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$DMG"
    xcrun stapler validate "$DMG"
    notarized=yes
else
    notarized=no
fi

rm -rf "$DIST/stage-macos"
echo "$DMG (signed by: $identity; notarized: $notarized)"
