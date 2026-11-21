#!/bin/bash
# The Apple apps' icon sets, from one 1024 px picture: the Mac app's AppIcon (every size from 16 to
# 1024 px) and the iPad and iPhone app's (one 1024 px opaque picture; Xcode makes the rest).
#
#   native/apple/Icons/make-icons.sh                 the PLACEHOLDER (a plum tile with a node glyph)
#   native/apple/Icons/make-icons.sh MASTER.png      the real icon: one 1024 px square picture
#   MAC_MASTER=M.png IOS_MASTER=I.png make-icons.sh  a different picture for each platform
#
# The Mac's picture is used as given (draw the rounded tile and its clear surround into it, as the
# Mac's icon grid has it). The iOS picture is made opaque over plum (the App Store refuses alpha);
# iOS rounds its corners itself, so draw it full bleed. The placeholder must be replaced, or approved
# by the maintainer, before a build goes to anyone.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
mac_set="$here/../PlumCraftMac/App/Assets.xcassets/AppIcon.appiconset"
ios_set="$here/../PlumCraftPad/App/Assets.xcassets/AppIcon.appiconset"
work="$(mktemp -d "${TMPDIR:-/tmp}/plumcraft-icons.XXXXXX")"
trap 'rm -rf "$work"' EXIT

mac_master="${MAC_MASTER:-${1:-}}"
ios_master="${IOS_MASTER:-${1:-}}"
if [ -z "$mac_master" ]; then swift "$here/icon.swift" placeholder-mac "$work/mac.png"; mac_master="$work/mac.png"; fi
if [ -z "$ios_master" ]; then swift "$here/icon.swift" placeholder-ios "$work/ios.png"; ios_master="$work/ios.png"; fi
for m in "$mac_master" "$ios_master"; do
  w=$(sips -g pixelWidth "$m" | awk '/pixelWidth/ {print $2}'); h=$(sips -g pixelHeight "$m" | awk '/pixelHeight/ {print $2}')
  [ "$w" = 1024 ] && [ "$h" = 1024 ] || { echo "make-icons: $m is ${w}x${h}, not 1024x1024" >&2; exit 2; }
done

mkdir -p "$mac_set" "$ios_set"
rm -f "$mac_set"/*.png "$ios_set"/*.png
images=""
for spec in 16:1 16:2 32:1 32:2 128:1 128:2 256:1 256:2 512:1 512:2; do
  pt=${spec%:*}; scale=${spec#*:}; px=$((pt * scale))
  suffix=""; if [ "$scale" = 2 ]; then suffix="@2x"; fi
  name="icon_${pt}x${pt}${suffix}.png"
  sips -z "$px" "$px" "$mac_master" --out "$mac_set/$name" >/dev/null
  images="$images{\"idiom\":\"mac\",\"size\":\"${pt}x${pt}\",\"scale\":\"${scale}x\",\"filename\":\"$name\"},"
done
printf '{"images":[%s],"info":{"author":"make-icons.sh","version":1}}\n' "${images%,}" | python3 -m json.tool > "$mac_set/Contents.json"

swift "$here/icon.swift" opaque "$ios_master" "$ios_set/icon_1024.png"
cat > "$ios_set/Contents.json" <<'JSON'
{
  "images" : [
    { "idiom" : "universal", "platform" : "ios", "size" : "1024x1024", "filename" : "icon_1024.png" }
  ],
  "info" : { "author" : "make-icons.sh", "version" : 1 }
}
JSON
for s in "$mac_set/.." "$ios_set/.."; do
  printf '{\n  "info" : { "author" : "make-icons.sh", "version" : 1 }\n}\n' > "$s/Contents.json"
done
echo "icons written: $mac_set (10 sizes), $ios_set (1024, opaque)"
