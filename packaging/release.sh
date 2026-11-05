#!/usr/bin/env bash
# Run the gate, build this machine's images, and optionally a Linux and a Windows host's, then
# write checksums. Nothing leaves this machine without --upload.
#
#   packaging/release.sh [--linux-host HOST] [--windows-host HOST [--windows-jump JUMP]]
#                        [--skip-gate] [--upload] [--publish]
#
# --linux-host HOST  also build on HOST over ssh, from a `git archive` of HEAD.
# --windows-host HOST  also build on Windows HOST (OpenSSH, PowerShell, Git for Windows, Rust,
#                    uv, Inno Setup) the same way. --windows-jump JUMP reaches it through JUMP
#                    (ssh JUMP, then ssh HOST from there), for a VM only its host can reach.
# --skip-gate        don't run the tests first.
# --upload           push the tag app-vVERSION and make a draft GitHub release with the images.
# --publish          with --upload, publish it instead of leaving a draft.
#
# The tag is app-vVERSION because vVERSION tags are the Python SDK's, which has its own version.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

linux_host='' windows_host='' windows_jump='' gate=yes upload='' publish=''
while [[ $# -gt 0 ]]; do
    case $1 in
        --linux-host) linux_host=${2:?--linux-host needs a host}; shift ;;
        --skip-gate) gate='' ;;
        --upload) upload=yes ;;
        --publish) publish=yes ;;
        --windows-host) windows_host=${2:?--windows-host needs a host}; shift ;;
        --windows-jump) windows_jump=${2:?--windows-jump needs a host}; shift ;;
        *) sed -n '2,16p' "${BASH_SOURCE[0]}" >&2; exit 2 ;;
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

if [[ -n $windows_host ]]; then
    # A command on the Windows host (its shell is PowerShell), through the jump host if any.
    win() {
        if [[ -n $windows_jump ]]; then
            # shellcheck disable=SC2029 # meant to expand here
            ssh "$windows_jump" "ssh $windows_host $(printf '%q' "$1")"
        else
            ssh "$windows_host" "$1"
        fi
    }
    # Copy a file to (to:PATH) or from (from:PATH) the Windows host's home, through the jump host.
    wincp() {
        local dir=$1 src=$2 dst=$3 hop="plumcraft-release-hop-$VERSION"
        if [[ -z $windows_jump ]]; then
            if [[ $dir == to ]]; then scp -q "$src" "$windows_host:$dst"; else scp -q "$windows_host:$src" "$dst"; fi
        elif [[ $dir == to ]]; then
            scp -q "$src" "$windows_jump:$hop" && ssh "$windows_jump" "scp -q $hop $windows_host:$dst && rm $hop"
        else
            ssh "$windows_jump" "scp -q $windows_host:$src $hop" && scp -q "$windows_jump:$hop" "$dst" && ssh "$windows_jump" "rm $hop"
        fi
    }
    remote="plumcraft-release-$VERSION"
    tree=$(mktemp -d)
    # Links dereferenced: an unprivileged Windows can't make them, and the crates share files by them.
    git archive --format=tar HEAD | tar -xC "$tree"
    tar -chf "$tree.tar" -C "$tree" .
    rm -rf "$tree"
    wincp to "$tree.tar" "$remote.tar"
    rm -f "$tree.tar"
    win "Remove-Item -Recurse -Force $remote -ErrorAction SilentlyContinue; New-Item -ItemType Directory $remote | Out-Null; tar -xf $remote.tar -C $remote; Remove-Item $remote.tar"
    win "& 'C:\Program Files\Git\bin\bash.exe' -lc 'cd ~/$remote && packaging/windows.sh'"
    for f in "plumcraft-$VERSION-windows-x86_64.zip" "PlumCraft-$VERSION-windows-x86_64-setup.exe"; do
        wincp from "$remote/dist/$f" "$DIST/$f"
    done
    win "Remove-Item -Recurse -Force $remote"
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
