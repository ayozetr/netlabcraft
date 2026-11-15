#!/usr/bin/env bash
# Build and package NetLabCraft for FreeBSD (run it on FreeBSD; CI runs it in a VM):
#
#   $DIST/netlabcraft-<version>-freebsd-x86_64.pkg      installable with `pkg add`
#   $DIST/netlabcraft-<version>-freebsd-x86_64.tar.gz   a /usr/local-style tree (bin/, share/)
#
# bin/ holds netlabcraft and, once apps/netlabcraft-cli exists, netlabcraft-cli. The desktop
# entry, AppStream metainfo, MIME type and icons are the freedesktop files packaging/linux ships.
# The package is not signed.
#
# Usage: packaging/freebsd/package.sh [--skip-build] [--install-test]
#   --install-test   install the finished .pkg with pkg(8) and run the CLI from /usr/local
#                    (changes the host; for the CI VM)
#
# Needs: bash, cargo, and the packages the release workflow's `freebsd` job installs.
# The VM-based job follows the pattern of the storytold Crafting Apps' FreeBSD packaging.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
LINUX="$ROOT/packaging/linux"
APP_ID=io.github.ayozetr.netlabcraft
ORIGIN=net/netlabcraft
PREFIX=/usr/local
# Runtime packages the app loads at start-up: winit and wgpu open these libraries with dlopen.
RUNTIME_DEPS=(libxkbcommon wayland vulkan-loader mesa-dri mesa-libs libX11 libxcb libXcursor libXrandr libXi shared-mime-info desktop-file-utils)

SKIP_BUILD=0
INSTALL_TEST=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --install-test) INSTALL_TEST=1; shift ;;
    -h | --help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ "$(uname -s)" = FreeBSD ]] || { echo "error: build on FreeBSD" >&2; exit 2; }
# FreeBSD's uname says amd64; release file names use x86_64 like the Linux and Windows ones.
case "$(uname -m)" in
  amd64 | x86_64) ARCH=x86_64 ;;
  arm64 | aarch64) ARCH=aarch64 ;;
  *) echo "unsupported architecture $(uname -m)" >&2; exit 2 ;;
esac
BASENAME="netlabcraft-$VERSION-freebsd-$ARCH"
MAINTAINER="${NETLABCRAFT_MAINTAINER:-NetLabCraft maintainers <53948812+ayozetr@users.noreply.github.com>}"

echo "==> NetLabCraft $VERSION for FreeBSD $ARCH (${BINS[*]})"

# Full parallelism on the biggest crates can run a small VM out of memory.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
if [[ "$SKIP_BUILD" = 0 ]]; then
  (cd "$ROOT" && cargo build --release --locked "${CARGO_PKGS[@]}")
fi
BIN="$CARGO_TARGET_DIR/release"
WORK="$CARGO_TARGET_DIR/freebsd-package"
TREE="$WORK/$BASENAME"     # the /usr/local-style tree, also the tarball's content
PKGROOT="$WORK/pkgroot"    # the same tree under $PREFIX, for pkg create
META="$WORK/meta"
rm -rf "$WORK"
mkdir -p "$TREE" "$META"

# ---- stage a /usr/local-style tree --------------------------------------------------------------
# FreeBSD's install(1) has no -D: create the directories first.
mkdir -p "$TREE/bin" "$TREE/share/applications" "$TREE/share/metainfo" \
  "$TREE/share/mime/packages" "$TREE/share/icons" "$TREE/share/doc/netlabcraft"
for b in "${BINS[@]}"; do
  install -m 755 "$BIN/$b" "$TREE/bin/"
  strip "$TREE/bin/$b" 2>/dev/null || true
done
install -m 644 "$LINUX/$APP_ID.desktop" "$TREE/share/applications/$APP_ID.desktop"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$NETLABCRAFT_BUILD_DATE/g" \
  "$LINUX/$APP_ID.metainfo.xml.in" >"$TREE/share/metainfo/$APP_ID.metainfo.xml"
install -m 644 "$LINUX/netlabcraft-mime.xml" "$TREE/share/mime/packages/netlabcraft-mime.xml"
cp -R "$ROOT/assets/app-icon/hicolor" "$TREE/share/icons/"
copy_docs "$TREE/share/doc/netlabcraft"

for f in "${BINS[@]/#/bin/}" "share/applications/$APP_ID.desktop" "share/metainfo/$APP_ID.metainfo.xml" \
  share/mime/packages/netlabcraft-mime.xml share/icons/hicolor/256x256/apps/netlabcraft.png \
  share/doc/netlabcraft/LICENSE-MIT share/doc/netlabcraft/LICENSE-APACHE; do
  [[ -e "$TREE/$f" ]] || { echo "error: $f is missing from the package" >&2; exit 1; }
done
if command -v desktop-file-validate >/dev/null; then desktop-file-validate "$TREE/share/applications/$APP_ID.desktop"; fi

# ---- .tar.gz ------------------------------------------------------------------------------------
tar -C "$WORK" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
echo "wrote $DIST/$BASENAME.tar.gz"

# ---- .pkg ---------------------------------------------------------------------------------------
mkdir -p "$PKGROOT$PREFIX"
cp -R "$TREE/." "$PKGROOT$PREFIX/"
# One plist line per file, relative to the prefix.
(cd "$PKGROOT$PREFIX" && find bin share -type f | sort) >"$META/plist"
echo "@dir share/doc/netlabcraft" >>"$META/plist"

# Dependencies are named by pkg with their origin and installed version, read from the host.
deps_json=""
for dep in "${RUNTIME_DEPS[@]}"; do
  info="$(pkg query '%o %v' "$dep" 2>/dev/null | head -n 1)"
  [[ -n "$info" ]] || { echo "error: runtime dependency $dep is not installed on the build host" >&2; exit 1; }
  deps_json="${deps_json:+$deps_json,}\"$dep\":{\"origin\":\"${info%% *}\",\"version\":\"${info#* }\"}"
done

cat >"$META/+MANIFEST" <<MANIFEST
{
  "name": "netlabcraft",
  "origin": "$ORIGIN",
  "version": "$VERSION",
  "comment": "Network simulator for teaching and lab work",
  "desc": "NetLabCraft is an open-source network simulator: build a network, configure routers and switches by typing the commands you learn in class, and watch every frame cross it. Includes the headless netlabcraft-cli.",
  "maintainer": "$MAINTAINER",
  "www": "https://github.com/ayozetr/netlabcraft",
  "abi": "$(pkg config ABI)",
  "prefix": "$PREFIX",
  "licenselogic": "or",
  "licenses": ["MIT", "APACHE20"],
  "categories": ["net", "education"],
  "deps": {$deps_json},
  "scripts": {
    "post-install": "update-mime-database $PREFIX/share/mime >/dev/null 2>&1 || true\nupdate-desktop-database $PREFIX/share/applications >/dev/null 2>&1 || true",
    "post-deinstall": "update-mime-database $PREFIX/share/mime >/dev/null 2>&1 || true\nupdate-desktop-database $PREFIX/share/applications >/dev/null 2>&1 || true"
  }
}
MANIFEST

mkdir -p "$WORK/out"
pkg create -r "$PKGROOT" -m "$META" -p "$META/plist" -o "$WORK/out"
made="$(find "$WORK/out" -maxdepth 1 -type f -name 'netlabcraft-*' | head -n 1)"
[[ -n "$made" ]] || { echo "error: pkg create wrote nothing" >&2; exit 1; }
mv "$made" "$DIST/$BASENAME.pkg"
echo "wrote $DIST/$BASENAME.pkg"
pkg info -F "$DIST/$BASENAME.pkg"

if [[ "$INSTALL_TEST" = 1 ]]; then
  echo "==> install test"
  pkg add "$DIST/$BASENAME.pkg"
  pkg info -l netlabcraft
  "$PREFIX/bin/$(smoke_bin)" --version
  pkg delete -y netlabcraft
fi

"$TREE/bin/$(smoke_bin)" --version
echo "==> done"
ls -lh "$DIST"
