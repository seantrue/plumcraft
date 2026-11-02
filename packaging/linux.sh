#!/usr/bin/env bash
# Build a tarball and a .deb for this machine's architecture, into dist/.
#
#   packaging/linux.sh
#
# Needs a Rust toolchain, pkg-config and ALSA's headers (Debian: libasound2-dev). The .deb needs
# dpkg-deb, and is skipped with a warning without it.
#
# SKIP_BUILD=1       use the binaries already in target/release.
# STAGE_ONLY=1       lay the images out in dist/stage-linux and stop; any OS, for checking them.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

if [[ $(uname -s) != Linux && -z ${STAGE_ONLY:-} ]]; then
    echo "linux.sh runs on Linux" >&2
    exit 2
fi

VERSION=$(version)
ARCH=$(uname -m)
STAGE="$DIST/stage-linux"
NAME="plumcraft-$VERSION-linux-$ARCH"

[[ -n ${SKIP_BUILD:-} ]] || build_binaries

rm -rf "$STAGE"
mkdir -p "$STAGE"

# Link with `plumcraft-editor` on PATH: the editor finds its engines beside its real path.
install_tree() {
    local prefix=$1
    mkdir -p "$prefix/lib/plumcraft" "$prefix/bin" "$prefix/share/applications" \
        "$prefix/share/icons/hicolor/1024x1024/apps" "$prefix/share/doc/plumcraft"
    copy_binaries "$prefix/lib/plumcraft"
    ln -s ../lib/plumcraft/plumcraft-editor "$prefix/bin/plumcraft-editor"
    ln -s ../lib/plumcraft/plumcraft "$prefix/bin/plumcraft"
    cp "$ROOT/packaging/plumcraft.desktop" "$prefix/share/applications/plumcraft.desktop"
    cp "$ROOT/assets/app-icon/plumcraft.png" "$prefix/share/icons/hicolor/1024x1024/apps/plumcraft.png"
    write_licences "$prefix/share/doc/plumcraft"
}

# Unpacks and runs from anywhere: bin/plumcraft-editor.
make_tarball() {
    mkdir -p "$STAGE/tarball"
    install_tree "$STAGE/tarball/$NAME"
}

make_deb() {
    local deb_arch root="$STAGE/deb"
    deb_arch=$(dpkg --print-architecture 2>/dev/null || echo "$ARCH")
    mkdir -p "$root/DEBIAN"
    install_tree "$root/usr"
    cat >"$root/DEBIAN/control" <<CONTROL
Package: plumcraft
Version: $VERSION
Section: graphics
Priority: optional
Architecture: $deb_arch
Depends: libc6, libgcc-s1, libasound2 | libasound2t64
Recommends: libvulkan1, libxkbcommon-x11-0, libwayland-client0
Maintainer: Sean D. True
Homepage: https://github.com/seantrue/plumcraft
Description: A node-flow editor with interchangeable engines
 PlumCraft is a native node-flow editor. Engines run compiled in, as separate
 processes or over HTTP. This package has the editor, the command line and the
 text, array, audio and craft engines.
CONTROL
}

make_tarball
make_deb

if [[ -n ${STAGE_ONLY:-} ]]; then
    echo "$STAGE"
    exit 0
fi

tar -C "$STAGE/tarball" -czf "$DIST/$NAME.tar.gz" "$NAME"
echo "$DIST/$NAME.tar.gz"

if command -v dpkg-deb >/dev/null; then
    dpkg-deb --root-owner-group --build "$STAGE/deb" "$DIST/$NAME.deb" >/dev/null
    echo "$DIST/$NAME.deb"
else
    echo "warning: no dpkg-deb, so no .deb" >&2
fi

rm -rf "$STAGE"
