#!/bin/zsh
# Regenerate the app icon from scripts/icon/icon.html (the drawing, on a
# canvas) into crates/onyx-app/assets/icon: the PNGs as the rounded app
# icon with its margin and shadow, icon.ico for Windows and, on macOS,
# icon.icns as the full square through iconutil, and the iPhone's and
# iPad's icon, the full square with no alpha channel, into the asset
# catalog at crates/onyx-app/ios. The page is run by WebKit
# (scripts/icon/render.swift), which draws every size on its own pixels, so
# this runs on a Mac; nothing else is needed.
#
#   scripts/make-icon.sh [--menubar]
#
# --menubar also writes the menu bar's template image, for a product with a
# menu bar item, to crates/onyx-app/assets/icons: menubar.png at 36 px
# (18 pt at 2x) and menubar-1x.png at 18.
set -euo pipefail
cd "$(dirname "$0")/.."
SRC=scripts/icon/icon.html
OUT=crates/onyx-app/assets/icon
BAR=crates/onyx-app/assets/icons
menubar=0
[[ "${1:-}" == --menubar ]] && menubar=1
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
SET="$TMP/icon.iconset"; mkdir -p "$SET" "$OUT"
jobs=()
for s in 1024 512 256 128 32; do jobs+=("$OUT/icon-$s.png:$s:margin"); done
for s in 256 128 64 48 32 16; do jobs+=("$TMP/ico-$s.png:$s:margin"); done
for s in 16 32 128 256 512; do
  jobs+=("$SET/icon_${s}x${s}.png:$s:bleed" "$SET/icon_${s}x${s}@2x.png:$((s * 2)):bleed")
done
jobs+=("crates/onyx-app/ios/Assets.xcassets/AppIcon.appiconset/icon-1024.png:1024:opaque")
if (( menubar )); then
  mkdir -p "$BAR"
  jobs+=("$BAR/menubar.png:36:template" "$BAR/menubar-1x.png:18:template")
fi
swift scripts/icon/render.swift "$SRC" "${jobs[@]}"
python3 scripts/icon/ico.py "$OUT/icon.ico" "$TMP"/ico-{256,128,64,48,32,16}.png
if command -v iconutil >/dev/null; then
  iconutil -c icns "$SET" -o "$OUT/icon.icns"
fi
ls -la "$OUT"
(( menubar )) && ls -la "$BAR"/menubar*.png
exit 0
