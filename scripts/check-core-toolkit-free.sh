#!/bin/sh
# The editor core and the media crate name no GUI toolkit (docs/research/24-native-shells.md
# section 3, rule 1): plumcraft-editor-core and plumcraft-media must not depend on egui, eframe,
# egui-snarl, winit, wgpu, rfd or plumcraft-ui, directly or through anything they build on, with
# every feature on, for any target.
set -u
cd "$(dirname "$0")/.."
status=0
for crate in plumcraft-editor-core plumcraft-media; do
  tree=$(cargo tree --locked -p "$crate" --all-features --target all -e normal,build --prefix none) || { echo "core gate: cargo tree failed for $crate"; exit 1; }
  bad=$(printf '%s\n' "$tree" | grep -E '^(egui|egui-snarl|egui_extras|egui_plot|egui_kittest|eframe|epaint|winit|wgpu|wgpu-[a-z]+|rfd|plumcraft-ui) v' | sort -u)
  if [ -n "$bad" ]; then
    echo "core gate: $crate pulls a GUI toolkit:"
    echo "$bad"
    status=1
  fi
done
[ "$status" -eq 0 ] && echo "core gate: ok (plumcraft-editor-core and plumcraft-media build no GUI toolkit)"
exit "$status"
