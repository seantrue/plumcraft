#!/bin/sh
# The editor core names no GUI toolkit (docs/research/24-native-shells.md section 3, rule 1):
# plumcraft-editor-core must not depend on egui, eframe, egui-snarl, winit, wgpu, rfd or
# plumcraft-ui, directly or through anything it builds on, with every feature on, for any target.
set -u
cd "$(dirname "$0")/.."
tree=$(cargo tree --locked -p plumcraft-editor-core --all-features --target all -e normal,build --prefix none) || { echo "core gate: cargo tree failed"; exit 1; }
bad=$(printf '%s\n' "$tree" | grep -E '^(egui|egui-snarl|egui_extras|egui_plot|egui_kittest|eframe|epaint|winit|wgpu|wgpu-[a-z]+|rfd|plumcraft-ui) v' | sort -u)
if [ -n "$bad" ]; then
  echo "core gate: plumcraft-editor-core pulls a GUI toolkit:"
  echo "$bad"
  exit 1
fi
echo "core gate: ok (plumcraft-editor-core builds no GUI toolkit)"
