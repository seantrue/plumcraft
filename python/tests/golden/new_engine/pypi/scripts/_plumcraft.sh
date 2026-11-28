# Finds PlumCraft's Rust binaries: $PLUMCRAFT_BIN, then PATH, then $PLUMCRAFT_HOME/target/{release,debug}.
# PLUMCRAFT_HOME defaults to the checkout the SDK came from, when pyproject.toml has a path.

find_plumcraft() {
    local name=$1 home dir
    if [[ -n "${PLUMCRAFT_BIN:-}" && -x "$PLUMCRAFT_BIN/$name" ]]; then
        echo "$PLUMCRAFT_BIN/$name"
        return
    fi
    if command -v "$name" >/dev/null; then
        command -v "$name"
        return
    fi
    home=${PLUMCRAFT_HOME:-$(sed -n 's/^plumcraft-engine = { path = "\(.*\)\/python".*/\1/p' pyproject.toml)}
    for dir in release debug; do
        if [[ -n "$home" && -x "$home/target/$dir/$name" ]]; then
            echo "$home/target/$dir/$name"
            return
        fi
    done
    echo "no $name: cargo install plumcraft plumcraft-editor, or set PLUMCRAFT_HOME to a PlumCraft checkout" >&2
    exit 2
}
