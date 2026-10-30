#!/usr/bin/env bash
# Installs NetLabCraft for the current user (no root): the binaries in ~/.local/bin, the menu
# entry, the icons and the .nlcraft file type. Builds the release binaries first if needed.
#
#   packaging/linux/install.sh            install (or update)
#   packaging/linux/install.sh --remove   uninstall
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN="$HOME/.local/bin"

if [[ "${1:-}" == "--remove" ]]; then
  rm -f "$BIN/netlabcraft" "$BIN/netlabcraft-cli" "$DATA/applications/netlabcraft.desktop" "$DATA/mime/packages/netlabcraft-mime.xml"
  find "$DATA/icons/hicolor" -name 'netlabcraft.*' -delete 2>/dev/null || true
  command -v update-mime-database >/dev/null && update-mime-database "$DATA/mime" || true
  command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true
  echo "NetLabCraft removed."
  exit 0
fi

if [[ ! -x "$ROOT/target/release/netlabcraft" || ! -x "$ROOT/target/release/netlabcraft-cli" ]]; then
  (cd "$ROOT" && cargo build --release -p netlabcraft -p netlabcraft-cli)
fi
install -Dm755 "$ROOT/target/release/netlabcraft" "$BIN/netlabcraft"
install -Dm755 "$ROOT/target/release/netlabcraft-cli" "$BIN/netlabcraft-cli"
install -Dm644 "$ROOT/packaging/linux/netlabcraft.desktop" "$DATA/applications/netlabcraft.desktop"
install -Dm644 "$ROOT/packaging/linux/netlabcraft-mime.xml" "$DATA/mime/packages/netlabcraft-mime.xml"
for d in "$ROOT"/assets/app-icon/hicolor/*/apps; do
  size="$(basename "$(dirname "$d")")"
  for f in "$d"/netlabcraft.*; do
    install -Dm644 "$f" "$DATA/icons/hicolor/$size/apps/$(basename "$f")"
  done
done
command -v update-mime-database >/dev/null && update-mime-database "$DATA/mime" || true
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$DATA/icons/hicolor" || true
echo "NetLabCraft installed: run it from the menu, or as netlabcraft (make sure $BIN is on your PATH)."
