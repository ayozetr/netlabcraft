#!/usr/bin/env bash
# Build and package the macOS release artifacts:
#
#   $DIST/netlabcraft-<version>-macos-<arch>.dmg          NetLabCraft.app on a drag-to-Applications DMG
#   $DIST/netlabcraft-cli-<version>-macos-<arch>.zip      the headless CLI
#
# Usage: packaging/macos/package.sh [--arch universal|aarch64|x86_64] [--skip-build]
#
# Signing (env): MACOS_SIGN_IDENTITY, a codesign identity (name or SHA-1). Default "-" is an
# ad-hoc signature: it keeps the binaries launchable on the Mac that built them, but Gatekeeper
# asks other Macs for confirmation (right-click > Open). Releases are not notarized yet.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/macos"

ARCH=universal
SKIP_BUILD=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$ARCH" in
  universal) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  aarch64) TARGETS=(aarch64-apple-darwin) ;;
  x86_64) TARGETS=(x86_64-apple-darwin) ;;
  *) echo "unknown --arch $ARCH" >&2; exit 2 ;;
esac

# Keep in sync with LSMinimumSystemVersion in Info.plist.in.
export MACOSX_DEPLOYMENT_TARGET=11.0
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
SHORT_VERSION="${VERSION%%-*}"
WORK="$CARGO_TARGET_DIR/macos-package"
APP="$WORK/NetLabCraft.app"
DMG="$DIST/netlabcraft-$VERSION-macos-$ARCH.dmg"
CLI_ZIP="$DIST/netlabcraft-cli-$VERSION-macos-$ARCH.zip"

[[ "$IDENTITY" = "-" ]] && warn "macOS: ad-hoc signing (no MACOS_SIGN_IDENTITY); the artifacts are not notarized"
echo "==> NetLabCraft $VERSION for macOS ($ARCH; ${BINS[*]}), identity: $IDENTITY"

# ---- build -------------------------------------------------------------------------------------
if [[ "$SKIP_BUILD" = 0 ]]; then
  args=()
  for t in "${TARGETS[@]}"; do args+=(--target "$t"); done
  (cd "$ROOT" && cargo build --release --locked "${CARGO_PKGS[@]}" "${args[@]}")
fi

rm -rf "$WORK"
mkdir -p "$WORK/bin"
for bin in "${BINS[@]}"; do
  inputs=()
  for t in "${TARGETS[@]}"; do inputs+=("$CARGO_TARGET_DIR/$t/release/$bin"); done
  lipo -create -output "$WORK/bin/$bin" "${inputs[@]}"
  lipo -info "$WORK/bin/$bin"
done

sign() {
  local ts=(--timestamp)
  [[ "$IDENTITY" = "-" ]] && ts=(--timestamp=none)
  codesign --force --sign "$IDENTITY" "${ts[@]}" "$@"
}

# ---- icon --------------------------------------------------------------------------------------
# Built here from the committed PNGs, so no .icns lives in the repository.
ICONSET="$WORK/netlabcraft.iconset"
mkdir -p "$ICONSET"
H="$ROOT/assets/app-icon/hicolor"
cp "$H/16x16/apps/netlabcraft.png" "$ICONSET/icon_16x16.png"
cp "$H/32x32/apps/netlabcraft.png" "$ICONSET/icon_16x16@2x.png"
cp "$H/32x32/apps/netlabcraft.png" "$ICONSET/icon_32x32.png"
cp "$H/64x64/apps/netlabcraft.png" "$ICONSET/icon_32x32@2x.png"
cp "$H/128x128/apps/netlabcraft.png" "$ICONSET/icon_128x128.png"
cp "$H/256x256/apps/netlabcraft.png" "$ICONSET/icon_128x128@2x.png"
cp "$H/256x256/apps/netlabcraft.png" "$ICONSET/icon_256x256.png"
cp "$H/512x512/apps/netlabcraft.png" "$ICONSET/icon_256x256@2x.png"
cp "$H/512x512/apps/netlabcraft.png" "$ICONSET/icon_512x512.png"
cp "$ROOT/assets/app-icon/netlabcraft-1024.png" "$ICONSET/icon_512x512@2x.png"
iconutil --convert icns --output "$WORK/netlabcraft.icns" "$ICONSET"

# ---- NetLabCraft.app ---------------------------------------------------------------------------
echo "==> assembling $APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
# The executable keeps the machine name (CFBundleExecutable); the bundle carries the display name.
cp "$WORK/bin/netlabcraft" "$APP/Contents/MacOS/netlabcraft"
if [[ "$HAS_CLI" = 1 ]]; then cp "$WORK/bin/netlabcraft-cli" "$APP/Contents/MacOS/netlabcraft-cli"; fi
cp "$WORK/netlabcraft.icns" "$APP/Contents/Resources/netlabcraft.icns"
mkdir -p "$APP/Contents/Resources/Licenses"
copy_docs "$APP/Contents/Resources/Licenses"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" \
  -e "s/@BUILD_SHA@/${NETLABCRAFT_BUILD_SHA:-unknown}/g" \
  "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"

# Sign inside-out: nested code first, then the bundle itself.
if [[ "$HAS_CLI" = 1 ]]; then
  sign --options runtime --identifier io.github.ayozetr.netlabcraft-cli "$APP/Contents/MacOS/netlabcraft-cli"
fi
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP/Contents/MacOS/netlabcraft"
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP"
codesign --verify --strict --deep --verbose=2 "$APP"

# ---- DMG ---------------------------------------------------------------------------------------
echo "==> building $DMG"
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/NetLabCraft.app"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
# `create` attaches a device while it copies, which fails now and then on CI runners ("Resource
# busy"), so it gets a few tries.
for attempt in 1 2 3 4 5; do
  if hdiutil create -srcfolder "$STAGE" -volname "NetLabCraft" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$DMG"; then break; fi
  [[ "$attempt" = 5 ]] && { echo "hdiutil create failed 5 times" >&2; exit 1; }
  echo "hdiutil create failed (attempt $attempt of 5); retrying" >&2
  sleep $((attempt * 10))
done
sign "$DMG"
codesign --verify --strict --verbose=2 "$DMG"

# ---- CLI ---------------------------------------------------------------------------------------
echo "==> building $CLI_ZIP"
CLI_DIR="$WORK/netlabcraft-cli-$VERSION-macos-$ARCH"
mkdir -p "$CLI_DIR"
cp "$WORK/bin/netlabcraft-cli" "$CLI_DIR/"
copy_docs "$CLI_DIR"
sign --options runtime --identifier io.github.ayozetr.netlabcraft-cli "$CLI_DIR/netlabcraft-cli"
rm -f "$CLI_ZIP"
ditto -c -k --keepParent "$CLI_DIR" "$CLI_ZIP"
"$WORK/bin/netlabcraft-cli" --version

echo "==> done"
ls -lh "$DIST"/netlabcraft*-"$VERSION"-macos-"$ARCH".*
