# Releasing NetLabCraft

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (GitHub Actions is back: CI runs; the release pipeline is next)

NetLabCraft has **no release yet**. GitHub Actions works again: `.github/workflows/ci.yml` runs
`cargo xtask ci` on every push to `main` and every pull request, and Dependabot proposes one grouped
dependency update a week (`.github/dependabot.yml`). The release workflow and the per-platform
packaging are the next step. This page records how releases will work, following the shared
craftrules release playbook (`release/playbook.md`) that the other Craft apps implement, and what is
missing before the first one.

## How it will work

- **Version:** the single source of truth is `[workspace.package] version` in the root
  `Cargo.toml` (now `0.1.0`); every crate inherits it. Read or change it with
  `cargo xtask version` and `cargo xtask version set X.Y.Z[-pre]`.
- **Gates:** a release commit passes `cargo xtask ci` (fmt, clippy, tests, doctests, assets,
  layers, wasm, parity) like every other commit.
- **Pipeline:** a push to a `release` branch runs a release workflow that builds installers for
  Linux, Windows, macOS, FreeBSD and the web, and creates a **draft** GitHub Release named
  `NetLabCraft v<version>`. Nobody sees a draft until a maintainer publishes it.
- **Names:** user-facing name **NetLabCraft**; binaries `netlabcraft` and `netlabcraft-cli`;
  files `netlabcraft-<version>-<platform>-<arch>.<ext>`.
- **File type:** `.nlcraft` (`application/x-netlabcraft+json`), registered by the installers.

## Before the first release

| Step | State | Estimate |
|---|---|---|
| App icon (original artwork) and `assets/app-icon/` with its row in `ATTRIBUTION.md` | ✅ `netlabcraft.png` (supplied by the owner) and its renderings (`packaging/icons.sh`); the window icon and the web favicon use it | — |
| `packaging/` (macOS bundle, Windows MSI, Linux `.desktop`/metainfo/Flatpak, FreeBSD, web) | 🟡 Linux: `.desktop`, the `.nlcraft` MIME type and a per-user `install.sh`. The rest waits for the app ID (reverse-DNS name), which depends on where the project will live; the siblings' packaging carries their own IDs and brand, so it is adapted, not copied as is | on hold |
| `.github/workflows/ci.yml` | ✅ runs `cargo xtask ci` on every push to `main` and every pull request | — |
| `.github/workflows/` release and packaging lint | ❌ not written yet; they need the packaging above | next |
| About window with version, licences and contributors ([`contributors.md`](contributors.md)) | ✅ About and Contributors tabs; run `cargo xtask contributors` right before tagging so the numbers are current | — |
| A public repository and a home for the project | 🟡 the repository is ready to go public (history scanned for secrets and personal data; CI, templates, security policy, contributing guide); making it public is the owner's step, after the release pipeline works | owner's decision |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | minor | GitHub Actions back: CI workflow and Dependabot added; releases no longer on hold, the release workflow and packaging are next. |
| 2026-10-30 | minor | Releases on hold by the owner's decision (no release for now; GitHub Actions not available); remaining steps no longer estimated. |
| 2026-10-30 | minor | About window lists contributors; refresh `contributors.json` before tagging. |
| 2026-10-30 | minor | App icon, window icon and favicon; Linux desktop entry, MIME type and per-user installer. |
| 2026-10-30 | major | Created: release plan and what the first release still needs. |
