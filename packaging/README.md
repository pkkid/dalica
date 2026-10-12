# Packaging

The scripts that turn a release build into the files on the Releases page. The release workflow
(`.github/workflows/release.yml`) calls them; each also runs by hand on its own platform. The
whole pipeline, its secrets and how to cut a release are in [`docs/releasing.md`](../docs/releasing.md).

| Script | Platform | Writes to `dist/release/` (or `$DIST`) |
|---|---|---|
| `linux/package.sh [--skip-build]` | Linux x86_64, aarch64 | `dalica-<v>-linux-<arch>.tar.gz` |
| `windows/package.ps1 -Arch x64 [-SkipBuild]` | Windows x64 | `dalica-<v>-windows-x64-portable.zip` |
| `macos/package.sh [--skip-build]` | macOS 11+ (universal) | `dalica-<v>-macos-universal.zip`, `dalica-cli-<v>-macos-universal.zip` |
| `checksums.sh create DIR` / `verify DIR` / `self-test` | any (bash) | `DIR/SHA256SUMS.txt` |
| `icons.sh [--check]` | Linux | the app icons in `assets/app-icon/` |

The three platform scripts are **stubs** for now (M6.13): each builds `dalica` and `dalica-cli` in
release with `--locked`, checks that both report the version and the commit (`--version`), and
packs them with `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `ATTRIBUTION.md`, `README.md`,
`OFL-Inter.txt` and, when `CRAFT_FONTS_DIR` is set, each craft-fonts family's `OFL-<family>.txt`.
M6.21 (Linux: FHS tree, AppImage, deb, rpm, Flatpak), M6.22 (Windows: MSI, icon, GUI subsystem)
and M6.23 (macOS: `Dalica.app`, DMG) replace the stubs; signing arrives in M15.

Every script reads the version from `[workspace.package] version` in the root `Cargo.toml` (or
`DALICA_VERSION`), and sets `DALICA_BUILD_SHA` (default: `git rev-parse HEAD`) and
`DALICA_BUILD_DATE` (default: today, UTC) for the build when they are not set already.

```sh
packaging/linux/package.sh                 # dist/release/dalica-0.0.0-linux-x86_64.tar.gz
packaging/checksums.sh create dist/release # dist/release/SHA256SUMS.txt
packaging/checksums.sh verify dist/release # exit 1 on any mismatch, missing or unlisted file
packaging/checksums.sh self-test           # plants mismatches and expects verify to fail
```

`checksums.sh verify` is strict: every listed file must exist with its hash, every file in the
folder must be listed, names are plain file names listed once. Users check a download with the
standard tools instead: `sha256sum -c SHA256SUMS.txt --ignore-missing` (Linux) or
`shasum -a 256 -c SHA256SUMS.txt --ignore-missing` (macOS).

The `Packaging lint` workflow (`.github/workflows/packaging-lint.yml`) checks everything here in
seconds on any change to `packaging/`, the workflows or the icons: actionlint, shellcheck, the
checksum self-test, a PowerShell parse, and xmllint, `desktop-file-validate`,
`appstreamcli validate` and the Flatpak manifests' agreement once those files exist.
