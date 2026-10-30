#!/usr/bin/env bash
# Installs PacketCraft for the current user (no root): the binaries in ~/.local/bin, the menu
# entry, the icons and the .pcraft file type. Builds the release binaries first if needed.
#
#   packaging/linux/install.sh            install (or update)
#   packaging/linux/install.sh --remove   uninstall
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN="$HOME/.local/bin"

if [[ "${1:-}" == "--remove" ]]; then
  rm -f "$BIN/packetcraft" "$BIN/packetcraft-cli" "$DATA/applications/packetcraft.desktop" "$DATA/mime/packages/packetcraft-mime.xml"
  find "$DATA/icons/hicolor" -name 'packetcraft.*' -delete 2>/dev/null || true
  command -v update-mime-database >/dev/null && update-mime-database "$DATA/mime" || true
  command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true
  echo "PacketCraft removed."
  exit 0
fi

if [[ ! -x "$ROOT/target/release/packetcraft" || ! -x "$ROOT/target/release/packetcraft-cli" ]]; then
  (cd "$ROOT" && cargo build --release -p packetcraft -p packetcraft-cli)
fi
install -Dm755 "$ROOT/target/release/packetcraft" "$BIN/packetcraft"
install -Dm755 "$ROOT/target/release/packetcraft-cli" "$BIN/packetcraft-cli"
install -Dm644 "$ROOT/packaging/linux/packetcraft.desktop" "$DATA/applications/packetcraft.desktop"
install -Dm644 "$ROOT/packaging/linux/packetcraft-mime.xml" "$DATA/mime/packages/packetcraft-mime.xml"
for d in "$ROOT"/assets/app-icon/hicolor/*/apps; do
  size="$(basename "$(dirname "$d")")"
  for f in "$d"/packetcraft.*; do
    install -Dm644 "$f" "$DATA/icons/hicolor/$size/apps/$(basename "$f")"
  done
done
command -v update-mime-database >/dev/null && update-mime-database "$DATA/mime" || true
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$DATA/icons/hicolor" || true
echo "PacketCraft installed: run it from the menu, or as packetcraft (make sure $BIN is on your PATH)."
