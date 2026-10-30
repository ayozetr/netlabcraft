#!/usr/bin/env bash
# Regenerate every app icon from assets/app-icon/packetcraft.svg.
#
# Needs rsvg-convert (librsvg) or resvg, and ImageMagick for the Windows .ico. The outputs are
# committed, so builds and packaging never need these tools.
#
#   packaging/icons.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/assets/app-icon"
SVG="$DIR/packetcraft.svg"
NAME="packetcraft"

render() {
  local size="$1" out="$2"
  if command -v rsvg-convert >/dev/null; then
    rsvg-convert -w "$size" -h "$size" "$SVG" -o "$out"
  elif command -v resvg >/dev/null; then
    resvg -w "$size" -h "$size" "$SVG" "$out" </dev/null
  else
    echo "error: rsvg-convert or resvg needed" >&2
    exit 1
  fi
}

render 1024 "$DIR/$NAME-1024.png"
# Linux hicolor theme (the 256 one is also the window icon).
for s in 16 32 48 64 128 256 512; do
  mkdir -p "$DIR/hicolor/${s}x${s}/apps"
  render "$s" "$DIR/hicolor/${s}x${s}/apps/$NAME.png"
done
mkdir -p "$DIR/hicolor/scalable/apps"
cp "$SVG" "$DIR/hicolor/scalable/apps/$NAME.svg"
# Windows: one .ico with the usual sizes.
if command -v magick >/dev/null; then
  magick "$DIR/hicolor/16x16/apps/$NAME.png" "$DIR/hicolor/32x32/apps/$NAME.png" "$DIR/hicolor/48x48/apps/$NAME.png" \
    "$DIR/hicolor/64x64/apps/$NAME.png" "$DIR/hicolor/128x128/apps/$NAME.png" "$DIR/hicolor/256x256/apps/$NAME.png" "$DIR/$NAME.ico"
else
  echo "warning: ImageMagick not found; $NAME.ico not regenerated" >&2
fi
echo "icons written to $DIR"
