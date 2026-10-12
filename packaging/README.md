# Packaging

The scripts that turn a release build into the files on the Releases page. The release workflow
(`.github/workflows/release.yml`) calls them; each also runs by hand on its own platform. The
whole pipeline, its secrets and how to cut a release are in [`docs/releasing.md`](../docs/releasing.md).

| Script | Platform | Writes to `dist/release/` (or `$DIST`) |
|---|---|---|
| `linux/package.sh [--skip-build] [--formats "tar deb rpm appimage flatpak"] [--install-tools] [--require-all]` | Linux x86_64, aarch64 | `dalica-<v>-linux-<arch>.tar.gz`, `.deb`, `.rpm`, `.AppImage`, `.AppImage.zsync`, `.flatpak` |
| `windows/package.ps1 -Arch x64 [-SkipBuild] [-Test\|-NoTest] [-InstallTools]` | Windows x64 | `dalica-<v>-windows-x64.msi`, `dalica-<v>-windows-x64-portable.zip` |
| `macos/package.sh [--skip-build]` | macOS 11+ (universal) | `dalica-<v>-macos-universal.dmg`, `dalica-cli-<v>-macos-universal.zip` |
| `checksums.sh create DIR` / `verify DIR` / `self-test` | any (bash) | `DIR/SHA256SUMS.txt` |
| `icons.sh [--check]` | Linux | the app icons in `assets/app-icon/` |

Beside them, each platform folder holds what its script uses and the checks it runs:

| Folder | Also holds |
|---|---|
| `linux/` | `tools.sh` (nfpm, appimagetool and the AppImage runtime, pinned with their SHA-256), `nfpm.yaml` (the `.deb` and `.rpm`), `org.dalica.Dalica.desktop`, `org.dalica.Dalica.metainfo.xml.in`, `flatpak/org.dalica.Dalica.bundle.yml`, `AppRun.sh` and its test `apprun-test.sh`, `postinst.sh` |
| `windows/` | `dalica.wxs` + `dalica.en-us.wxl` (the MSI, WiX v5), `sign.ps1`, `pe.ps1` (PE header checks), `test-build-rs.ps1`, `test-portable.ps1`, `test-msi.ps1`, the `portable` marker |
| `macos/` | `common.sh`, `Info.plist.in`, `entitlements.plist`, `dmg/generate.py` (the DMG layout, generated at every build), `verify.sh` (the finished DMG and CLI zip), `test.sh` (template and layout checks on any OS), `import-cert.sh` (the signing job's certificate) |

Each platform script builds `dalica` and `dalica-cli` in release with `--locked`, checks that both
report the version and the commit (`--version`), and ships `LICENSE-MIT`, `LICENSE-APACHE`,
`NOTICE`, `ATTRIBUTION.md`, `README.md`, `OFL-Inter.txt` and, when `CRAFT_FONTS_DIR` is set, each
craft-fonts family's `OFL-<family>.txt`. Locally the Linux script downloads nothing (a format
whose tool is missing is skipped and named; `--install-tools` installs the pinned tools,
`--require-all` makes a skip an error; both are on in GitHub Actions, `DALICA_INSTALL_TOOLS=0`
turns them off), the Windows script skips the MSI without WiX (`-InstallTools` installs it; its
tests run in CI or with `-Test`), and the macOS script signs ad hoc without
`MACOS_SIGN_IDENTITY`. What each checks is in [`docs/releasing.md`](../docs/releasing.md); signing
and notarization arrive in M15.

Every script reads the version from `[workspace.package] version` in the root `Cargo.toml` (or
`DALICA_VERSION`), and sets `DALICA_BUILD_SHA` (default: `git rev-parse HEAD`) and
`DALICA_BUILD_DATE` (default: today, UTC) for the build when they are not set already.

```sh
packaging/linux/package.sh                 # dist/release/dalica-0.1.0-linux-x86_64.{tar.gz,deb,...}
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
