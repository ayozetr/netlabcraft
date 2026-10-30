# Releasing PacketCraft

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (app icon, Linux desktop integration)

PacketCraft has **no release yet**. This page records how releases will work, following the
shared craftrules release playbook (`release/playbook.md`) that the other Craft apps implement,
and what is missing before the first one.

## How it will work

- **Version:** the single source of truth is `[workspace.package] version` in the root
  `Cargo.toml` (now `0.1.0`); every crate inherits it. Read or change it with
  `cargo xtask version` and `cargo xtask version set X.Y.Z[-pre]`.
- **Gates:** a release commit passes `cargo xtask ci` (fmt, clippy, tests, doctests, assets,
  layers, wasm, parity) like every other commit.
- **Pipeline:** a push to a `release` branch runs a release workflow that builds installers for
  Linux, Windows, macOS, FreeBSD and the web, and creates a **draft** GitHub Release named
  `PacketCraft v<version>`. Nobody sees a draft until a maintainer publishes it.
- **Names:** user-facing name **PacketCraft**; binaries `packetcraft` and `packetcraft-cli`;
  files `packetcraft-<version>-<platform>-<arch>.<ext>`.
- **File type:** `.pcraft` (`application/x-packetcraft+json`), registered by the installers.

## Before the first release

| Step | State | Estimate |
|---|---|---|
| App icon (original artwork) and `assets/app-icon/` with its row in `ATTRIBUTION.md` | ✅ `packetcraft.png` (supplied by the owner) and its renderings (`packaging/icons.sh`); the window icon and the web favicon use it | — |
| `packaging/` (macOS bundle, Windows MSI, Linux `.desktop`/metainfo/Flatpak, FreeBSD, web) | 🟡 Linux: `.desktop`, the `.pcraft` MIME type and a per-user `install.sh`. The rest waits for the app ID (reverse-DNS name), which depends on where the project will live; the siblings' packaging carries their own IDs and brand, so it is adapted, not copied as is | 5–8 h |
| `.github/workflows/` (ci, release, packaging lint) | ❌ | 3–5 h |
| About window with version, licences and contributors ([`contributors.md`](contributors.md)) | ❌ | 3–5 h |
| A public repository and a home for the project | ❌ | owner's decision |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | App icon, window icon and favicon; Linux desktop entry, MIME type and per-user installer. |
| 2026-10-30 | major | Created: release plan and what the first release still needs. |
