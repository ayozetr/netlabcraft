#!/usr/bin/env bash
# Regenerate every app icon from assets/app-icon/netlabcraft.png (the 1254 px master).
#
# Needs ImageMagick 7 (magick). The outputs are committed, so builds and packaging never need it.
#
#   packaging/icons.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/assets/app-icon"
SRC="$DIR/netlabcraft.png"
NAME="netlabcraft"

command -v magick >/dev/null || { echo "error: ImageMagick (magick) not found" >&2; exit 1; }

# Every output without metadata (no dates, no text chunks), so regenerating changes nothing.
render() {
  magick "$SRC" -filter Lanczos -resize "$1x$1" -strip -define png:exclude-chunks=date,time "$2"
}

render 1024 "$DIR/$NAME-1024.png"
# macOS icons sit on Apple's grid: an 824/1024 body with a transparent margin.
magick "$SRC" -filter Lanczos -resize 412x412 -background none -gravity center -extent 512x512 -strip \
  -define png:exclude-chunks=date,time "$DIR/$NAME-macos-512.png"
# Linux hicolor theme (the 256 one is also the window icon on Windows and Linux).
for s in 16 24 32 48 64 128 256 512; do
  mkdir -p "$DIR/hicolor/${s}x${s}/apps"
  render "$s" "$DIR/hicolor/${s}x${s}/apps/$NAME.png"
done
# Windows: one .ico with 16-256 px.
magick "$SRC" -strip -define icon:auto-resize=256,128,64,48,32,24,16 "$DIR/$NAME.ico"
echo "icons written to $DIR"
