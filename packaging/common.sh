# shellcheck shell=bash
# Shared by packaging/macos.sh, linux.sh and windows.sh; sourced, not run.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck disable=SC2034 # used by the scripts that source this
DIST=${PLUMCRAFT_DIST:-$ROOT/dist}
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
# Where the release binaries land: cargo builds for CARGO_BUILD_TARGET (a triple such as
# x86_64-apple-darwin) when it is set, into a directory of that name.
# shellcheck disable=SC2034 # used by the scripts that source this
RELEASE=$TARGET/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release
# Windows' executables end in .exe (windows.sh runs under Git for Windows' bash).
case $(uname -s) in MINGW* | MSYS* | CYGWIN*) EXE=.exe ;; *) EXE='' ;; esac
# The Python that runs third_party.py; windows.sh points it at a real one.
PYTHON=${PYTHON:-python3}

# What an image holds. The engines sit beside the editor, where it looks for them.
BINARIES=(plumcraft-editor plumcraft plumcraft-text-engine plumcraft-array-engine plumcraft-audio-engine plumcraft-craft-engine)
CRATES=(plumcraft-editor plumcraft plumcraft-text plumcraft-array plumcraft-audio plumcraft-craft)

# The workspace version, which the editor and the crates share.
version() {
    sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1
}

build_binaries() {
    local args=(build --release --locked --bins)
    local crate
    for crate in "${CRATES[@]}"; do
        args+=(-p "$crate")
    done
    (cd "$ROOT" && cargo "${args[@]}")
}

# Copy the built binaries into $1.
copy_binaries() {
    local name
    for name in "${BINARIES[@]}"; do
        cp "$RELEASE/$name$EXE" "$1/$name$EXE"
    done
}

# Everything a binary distribution must carry: PlumCraft's licences, the bundled fonts' (Inter Regular
# and SemiBold: one OFL 1.1 covers both) and the
# copied craft-libs modules', and every dependency's, into $1.
write_licences() {
    mkdir -p "$1"
    cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$1/"
    cp "$ROOT/crates/ui/assets/fonts/OFL-Inter.txt" "$1/"
    cp "$ROOT/crates/ui/src/craft_ui/NOTICE" "$1/NOTICE-craft-libs"
    "$PYTHON" "$ROOT/packaging/third_party.py" "$ROOT" "${CRATES[@]}" >"$1/THIRD-PARTY.txt"
}
