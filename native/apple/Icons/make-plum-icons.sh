#!/bin/bash
# The Apple apps' icons from the egui editor's plums (the maintainer, 2026-11-21: "Use the egui
# editor's plums, adapted"):
#
#   - the Mac: assets/app-icon/plumcraft.png as it is: a 1024 px picture holding the rounded tile
#     (824 px) on a clear surround with its shadow, the Mac's icon grid;
#   - iOS: a full-bleed 1024 px square with no alpha and no rounded corners drawn in (iOS masks it):
#     the largest square of the same tile that is wholly opaque, scaled up (full-bleed.swift), kept
#     as plums-ios-1024.png beside this script.
#
#   native/apple/Icons/make-plum-icons.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
src="$here/../../../assets/app-icon/plumcraft.png"
ios="$here/plums-ios-1024.png"
swift "$here/full-bleed.swift" "$src" "$ios"
MAC_MASTER="$src" IOS_MASTER="$ios" "$here/make-icons.sh"
