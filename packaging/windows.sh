#!/usr/bin/env bash
# Build a zip and an installer for Windows x86-64, into dist/. Runs under Git for Windows' bash:
#
#   "C:\Program Files\Git\bin\bash.exe" packaging/windows.sh
#
# Needs the MSVC Rust toolchain, uv (for a Python to write THIRD-PARTY.txt; or set PYTHON) and
# Inno Setup 6 (`winget install JRSoftware.InnoSetup`).
#
# ISCC=PATH          Inno Setup's compiler, if it isn't in one of its usual places.
# SKIP_BUILD=1       use the binaries already in target/release.
# STAGE_ONLY=1       lay the image out in dist/stage-windows and stop.

caller_python=${PYTHON:-}
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

case $(uname -s) in
    MINGW* | MSYS* | CYGWIN*) ;;
    *) echo "windows.sh runs under Git for Windows' bash" >&2; exit 2 ;;
esac

VERSION=$(version)
STAGE="$DIST/stage-windows"
NAME="plumcraft-$VERSION-windows-x86_64"
APPDIR="$STAGE/$NAME"

# The Python that writes the licences: uv's, since Windows' `python3` may be the Store's stub.
if [[ -z $caller_python ]]; then
    PYTHON=$(uv python find 2>/dev/null) || { echo "no Python: install uv, then 'uv python install'" >&2; exit 2; }
fi

iscc=${ISCC:-}
if [[ -z $iscc ]]; then
    for c in "${LOCALAPPDATA:-}/Programs/Inno Setup 6/ISCC.exe" "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" "/c/Program Files/Inno Setup 6/ISCC.exe"; do
        [[ -f $(cygpath -u "$c" 2>/dev/null || echo "$c") ]] && { iscc=$(cygpath -u "$c"); break; }
    done
fi

[[ -n ${SKIP_BUILD:-} ]] || build_binaries

rm -rf "$STAGE"
mkdir -p "$APPDIR/licences"
# One folder: the editor finds its engines beside it.
copy_binaries "$APPDIR"
cp "$ROOT/assets/app-icon/plumcraft.ico" "$APPDIR/plumcraft.ico"
write_licences "$APPDIR/licences"

if [[ -n ${STAGE_ONLY:-} ]]; then
    echo "$STAGE"
    exit 0
fi

# The zip unpacks and runs from anywhere: plumcraft-editor.exe.
rm -f "$DIST/$NAME.zip"
powershell -NoProfile -Command "Compress-Archive -Path '$(cygpath -w "$APPDIR")' -DestinationPath '$(cygpath -w "$DIST/$NAME.zip")'"
echo "$DIST/$NAME.zip"

if [[ -n $iscc ]]; then
    # MSYS2_ARG_CONV_EXCL: Git Bash would otherwise rewrite /Q and /D... into paths
    MSYS2_ARG_CONV_EXCL='*' "$iscc" /Q "/DVersion=$VERSION" "/DStage=$(cygpath -w "$APPDIR")" "/DOutputDir=$(cygpath -w "$DIST")" "$(cygpath -w "$ROOT/packaging/plumcraft.iss")"
    echo "$DIST/PlumCraft-$VERSION-windows-x86_64-setup.exe"
else
    echo "warning: no Inno Setup (ISCC.exe), so no installer: winget install JRSoftware.InnoSetup" >&2
fi

rm -rf "$STAGE"
