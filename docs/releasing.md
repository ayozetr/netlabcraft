# Releasing NetLabCraft

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (second release, 0.2.0) · earlier: major (the release pipeline exists and ran end to end; first release)

The recipe follows the shared Crafting Apps playbook (`release/playbook.md` in craftrules): a
release is whatever is on the `release` branch. Pushing to it makes `.github/workflows/release.yml`
build every platform and attach the files to a **draft** GitHub Release, which a maintainer
publishes once every job is green. Builds are **not code-signed or notarized yet** (see below).

`ci.yml` runs `cargo xtask ci` on every push to `main` and every pull request. A pull request that
touches `packaging/`, `rust-toolchain.toml` or the release workflow also builds every package as a
dry run, with no draft. Dependabot proposes one grouped dependency update a week
(`.github/dependabot.yml`).

## Cutting a release

1. Bump the version on `main`, through a pull request: `cargo xtask version set X.Y.Z` (the
   workspace version in the root `Cargo.toml` is the single source; `cargo xtask version` prints
   it). Run `cargo xtask contributors` so About ▸ Contributors is current.
2. Fast-forward `release` to `main` (`git push origin main:release`). The push runs the workflow,
   which builds every platform and creates the draft release **NetLabCraft vX.Y.Z** (a version with
   a `-pre` suffix is marked as a prerelease). The tag `vX.Y.Z` is created when the draft is
   published.
3. When every job is green, download a package or two and run them (the AppImage's `--cli
   --version`, the Windows `.msi` or portable zip, the macOS `.dmg`). Publish the draft.
4. After publishing, close the issues the release fixes, naming the version.

A dry run from any branch, building everything and publishing nothing:
`gh workflow run release.yml --ref <branch>` (the `dry-run` input defaults to true). A full test
that also creates a draft: `gh workflow run release.yml -f version=0.0.0-test.1 -f dry-run=false`,
then delete the draft with `gh release delete v0.0.0-test.1 --cleanup-tag`.

## What it ships

| Platform | Files |
|---|---|
| Linux | `netlabcraft-<v>-linux-{x86_64,aarch64}.{AppImage,AppImage.zsync,deb,rpm,tar.gz}` |
| Windows | `netlabcraft-<v>-windows-x64.msi` and `netlabcraft-<v>-windows-x64-portable.zip` |
| macOS | `netlabcraft-<v>-macos-universal.dmg` (`NetLabCraft.app`), `netlabcraft-cli-<v>-macos-universal.zip` |
| Web | `netlabcraft-web-<v>.zip` (the browser build, `apps/netlabcraft-web`) |

Plus `SHA256SUMS.txt`. Linux is built on Ubuntu 22.04 (glibc 2.35) so the binaries run on newer
distributions.

## App-specific choices

- **App id:** `io.github.ayozetr.netlabcraft` (desktop file, AppStream id, macOS bundle id, window
  `app_id`). The icon keeps the plain name `netlabcraft`.
- **Not signed or notarized.** Windows shows SmartScreen's warning, macOS asks for confirmation
  (right-click, Open) and the Linux packages are unsigned. Signing needs certificates the project
  does not have; `packaging/macos/package.sh` already accepts `MACOS_SIGN_IDENTITY`.
- **File type:** `.nlcraft` (`application/x-netlabcraft+json`) is registered on Linux (MIME XML) and
  macOS (`Info.plist`). The Windows installer does not claim it yet.
- **The WiX UpgradeCode** `901628DA-E212-4FAE-A025-505251BFCFA7` belongs to NetLabCraft for good.
- **Not shipped yet:** FreeBSD, Flatpak, riscv64, Windows ARM64 and the Arch package.
- **Fonts and icons:** the app uses egui's default fonts (listed in `ATTRIBUTION.md`, which every
  package ships) and original icons; nothing is downloaded at build time.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | minor | Second release, 0.2.0, cut with the flow above. |
| 2026-11-13 | major | Release pipeline written and run end to end: Linux, Windows, macOS and web packages, SHA256SUMS and a draft release; first release. |
| 2026-11-13 | minor | GitHub Actions back: CI workflow and Dependabot added. |
| 2026-10-30 | minor | Releases on hold by the owner's decision; About window lists contributors; app icon and Linux per-user installer. |
| 2026-10-30 | major | Created: release plan and what the first release still needs. |
