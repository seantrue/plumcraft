#!/usr/bin/env bash
# Run the gate, build this machine's images, and optionally a Linux host's, then write checksums.
# Nothing leaves this machine without --upload.
#
#   packaging/release.sh [--linux-host HOST] [--skip-gate] [--upload] [--publish]
#
# --linux-host HOST  also build on HOST over ssh, from a `git archive` of HEAD.
# --skip-gate        don't run the tests first.
# --upload           push the tag app-vVERSION and make a draft GitHub release with the images.
# --publish          with --upload, publish it instead of leaving a draft.
#
# The tag is app-vVERSION because vVERSION tags are the Python SDK's, which has its own version.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

linux_host='' gate=yes upload='' publish=''
while [[ $# -gt 0 ]]; do
    case $1 in
        --linux-host) linux_host=${2:?--linux-host needs a host}; shift ;;
        --skip-gate) gate='' ;;
        --upload) upload=yes ;;
        --publish) publish=yes ;;
        *) sed -n '2,12p' "${BASH_SOURCE[0]}" >&2; exit 2 ;;
    esac
    shift
done
[[ -z $publish || -n $upload ]] || { echo "--publish needs --upload" >&2; exit 2; }

cd "$ROOT" || exit 2
VERSION=$(version)
TAG="app-v$VERSION"

[[ -z $(git status --porcelain) ]] || { echo "commit or stash first: the working tree is dirty" >&2; exit 2; }
if [[ -n $upload ]]; then
    git fetch -q origin
    [[ $(git rev-parse HEAD) == $(git rev-parse origin/main) ]] || { echo "--upload releases origin/main: HEAD isn't it" >&2; exit 2; }
    ! git rev-parse -q --verify "refs/tags/$TAG" >/dev/null || { echo "$TAG exists: bump the workspace version" >&2; exit 2; }
fi

if [[ -n $gate ]]; then
    cargo test --all-features
    cargo clippy --all-targets --all-features -- -D warnings
    cargo fmt --all --check
    (cd python && uv run pytest && uv run ruff check .)
fi

rm -rf "$DIST"
mkdir -p "$DIST"

case $(uname -s) in
    Darwin) packaging/macos.sh ;;
    Linux) packaging/linux.sh ;;
    *) echo "no images for $(uname -s)" >&2; exit 2 ;;
esac

if [[ -n $linux_host ]]; then
    # shellcheck disable=SC2029 # $remote is meant to expand here
    remote="plumcraft-release-$VERSION"
    git archive --format=tar HEAD | ssh "$linux_host" "rm -rf '$remote' && mkdir '$remote' && tar -xC '$remote'"
    ssh "$linux_host" "cd '$remote' && . \"\$HOME/.cargo/env\" 2>/dev/null; packaging/linux.sh"
    scp "$linux_host:$remote/dist/plumcraft-$VERSION-linux-*" "$DIST/"
    ssh "$linux_host" "rm -rf '$remote'"
fi

(cd "$DIST" && shasum -a 256 -- * >SHA256SUMS)
ls -l "$DIST"

if [[ -n $upload ]]; then
    git tag -a "$TAG" -m "PlumCraft $VERSION"
    git push origin "$TAG"
    args=(--verify-tag --title "PlumCraft $VERSION" --notes-file CHANGELOG.md)
    [[ -n $publish ]] || args+=(--draft)
    gh release create "$TAG" "${args[@]}" "$DIST"/*
fi
