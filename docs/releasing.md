# Releasing Dalica

Adapted from LightCraft `docs/releasing.md` (revision 88ea1a8) and PhotoCraft `docs/releasing.md`
and `docs/release-playbook.md` (revision 59684266); MIT OR Apache-2.0.

> Status: **The pipeline runs (M6.13), builds every package (M6.21–M6.23) and signs the Windows
> (M15.10) and macOS (M15.13) packages, with an SBOM per package and the RustSec advisory check
> (M15.40).** `.github/workflows/release.yml` and `packaging-lint.yml`
> run as described under *What runs now*, and `packaging/` builds the installers of *What gets
> built*, copied in shape from the Craft apps' release pipeline ([`process.md`](process.md) §7).
> Every dry run signs both platforms with throwaway identities; the real certificates, Apple's
> notarization and the stapled tickets need the maintainer's secrets and are first checked on the
> 1.0 release run (M15.R). Linux packages are not code-signed (*Secrets*). Update this page as the
> pieces land.

Every push to the `release` branch runs `.github/workflows/release.yml`. The workflow builds
installers for Linux, Windows and macOS, signs what it has credentials for, and creates or updates a
**draft** GitHub Release named `Dalica v<version>`. Nobody sees a draft until a maintainer publishes
it.

User-facing names say **Dalica**. Files, binaries and ids stay lowercase
(`dalica-<version>-<platform>-<arch>.<ext>`). The reverse-DNS application id (Wayland app id,
`.desktop` file, hicolor icon name, macOS bundle id) is `org.dalica.Dalica`
(`crates/core/src/app_id.rs`).

## Cutting a release

**When:** at every milestone review from M7 to M15 (maintainer rule, 2026-10-13): M7 ships 0.2,
M8 0.3, M9 0.4, M10 0.5, M11 0.6, M12 0.7, M13 0.8, M14 0.9 and M15 1.0. The review branch carries
steps 1's version and notes; after the maintainer merges it and signs off, the lead runs steps 2–6
(`/milestone-review` §5).

1. **Bump the version** on `main`. The only place it lives is `[workspace.package] version` in the
   root `Cargo.toml`:

   ```sh
   cargo xtask version                 # prints the current version, e.g. 0.1.0
   cargo xtask version set 0.2.0       # or 0.2.0-rc.1; updates Cargo.toml and Cargo.lock
   ```

   Use a semantic version; a version containing a hyphen (`0.2.0-rc.1`) is marked as a pre-release.
   Commit the change (`Cargo.toml` + `Cargo.lock`) through the normal review flow, together with the
   new section of [`whats-new.md`](whats-new.md).
2. **Merge `main` into `release`** (or fast-forward it) and push. The workflow starts by itself.
3. **Wait for the draft.** The Releases page gets a draft `Dalica v0.2.0`, tagged `v0.2.0` on the
   pushed commit, with every artifact, the four SBOMs (`*.cdx.json`) and `SHA256SUMS.txt`. The
   notes are generated from the merged PRs.
4. **Check it.** Download an installer or two, install them, open the procedural demo library,
   and read the job summaries. Any `::warning::` there means a signing secret was missing and that
   artifact is unsigned. The `SBOMs and advisories` summary lists the recorded advisories and the
   informational ones (unmaintained crates) to review.
5. **Add the honest deltas** to the draft's notes: what moved in [`parity.md`](parity.md) and the
   workflow-acceptance pass rate since the previous release, and any budget in
   [`development.md`](development.md) §8 newly met or missed (with the machine and date of the
   measurement). Never quote a wiring percentage as if it were parity
   ([`process.md`](process.md) §5).
6. **Publish** the draft in the GitHub UI. Publishing creates the tag.

Pushing to `release` again before you publish rebuilds the same draft and replaces its assets.
After the draft is published, the workflow refuses to touch that version again, so bump it first.

**Test runs:** *Actions → Release → Run workflow* runs the whole pipeline by hand, with an optional
`version` input (such as `0.2.0-rc.1`) that overrides `Cargo.toml` for that run only (applied with
`cargo xtask version set` before building, so the binaries report it too). Only a run on the
`release` branch signs with the real certificates and drafts; a run on any other branch, and every
pull request that touches `packaging/`, `.github/workflows/` or `assets/app-icon/`, is a **dry
run**: it builds, signs, checksums and verifies every platform, but the signing jobs run without the
`release` environment, never read its secrets and sign with throwaway self-signed identities they
make themselves (trusted on that runner only); macOS notarization and stapling are skipped and named
in the log and the job summary; the draft job is skipped. A pull request's own checks are therefore
a dry run of the release, signing path included.

## What runs now

`.github/workflows/release.yml`, since M6.13. The workflow defaults to `contents: read`, pins every
action to a full commit hash, builds with the pinned toolchain (`1.98`, as `ci.yml`) and
`--locked`, and passes inputs to scripts through environment variables only.

| Job | Runs on | Does | Dry run |
|---|---|---|---|
| `version` | ubuntu-latest | the version (the `version` input or `Cargo.toml`), checked like `cargo xtask version set`; pre-release when it has a hyphen; the build date; refuses a version whose release is already published | runs; a published version is only a warning |
| `build Linux x86_64`, `aarch64` | ubuntu-22.04, ubuntu-22.04-arm | `packaging/linux/package.sh` | runs |
| `build Windows x64` | windows-latest | `packaging/windows/package.ps1 -Arch x64` (static C runtime) | runs |
| `build macOS universal` | macos-latest | `packaging/macos/package.sh` (both architectures, `lipo`) | runs |
| `sign Windows x64` (M15.10) | windows-latest, **`release` environment** | `package.ps1 -FromStage`: signs both programs from the build job's stage, builds the MSI around them and signs it, zips the portable set again, checks every signature (`test-sign.ps1`); the `.pfx` or Azure Trusted Signing, a `::warning::` naming each unsigned file without either | runs without the environment; signs with a throwaway certificate (`sign.ps1 -Throwaway`) |
| `sign macOS universal` (M15.13) | macos-latest, **`release` environment** | `import-cert.sh` imports the Developer ID certificate into a temporary keychain; `package.sh --from-stage` assembles and signs `Dalica.app`, the DMG and the CLI zip from the build job's stage, notarizes all three (`notarize.sh`), staples the app and the DMG, and runs `verify.sh`; a `::warning::` and ad hoc signatures without the certificate, a warning and no notarization without all three Apple secrets; the keychain is deleted at the end whatever happened | runs without the environment; signs with a throwaway identity (`import-cert.sh --throwaway`); notarization, stapling and Gatekeeper's checks skipped and named |
| `SBOMs and advisories` (M15.40) | ubuntu-latest | `cargo xtask sbom --out dist/release`: one CycloneDX 1.5 SBOM per release package, `dalica-<v>-<package>.cdx.json` for `linux-x86_64`, `linux-aarch64`, `windows-x64` and `macos-universal`, each listing every crate the package's targets compile with its licence, purl, `.crate` SHA-256 and edges (nothing is built: `cargo metadata` and `cargo tree` resolve each target); then `cargo xtask advisories --fetch` checks those crates against the RustSec database fetched now, and an advisory `security.md` §8.4 does not record fails the run before any checksum is written; both results go to the job summary | runs, the same |
| `checksums` | ubuntu-latest | downloads the whole set (the SBOMs included) and writes `SHA256SUMS.txt` (`packaging/checksums.sh create`) | runs over the throwaway-signed set |
| `verify checksums` | ubuntu, Windows and macOS | on a fresh runner each: re-downloads the set and `SHA256SUMS.txt`, runs the script's planted-mismatch test (`checksums.sh self-test`) and `checksums.sh verify` | runs |
| `draft GitHub Release` | ubuntu-latest, **`contents: write`** | verifies the set once more, refuses a published version, then creates the draft `Dalica v<version>` tagged `v<version>` on the pushed commit with generated notes (or replaces an existing draft's assets), every artifact and `SHA256SUMS.txt` | skipped |

Every build job sets `DALICA_BUILD_SHA` (the commit) and `DALICA_BUILD_DATE` (the `version` job's
UTC date), and each script fails unless both binaries' `--version` reports the version and that
commit. The build jobs read no secrets and run without the `release` environment; the signing jobs
alone use it, and only in a release run (each secret is passed as
`real && secrets.X || ''`, so a dry run's environment holds empty strings); only the draft job may
write. The build jobs for Windows and macOS upload their tested programs and licence files as a
stage (`stage-<platform>-<arch>`, outside the `release-*` set the checksum and draft jobs take),
and the signing jobs package the signed set from it, so the checksums are written over what is
signed.

What each platform job uploads (sizes from the pull requests' dry runs of 0.0.0):

| Platform | Artifacts | Size | Since |
|---|---|---|---|
| Linux x86_64, aarch64 | `dalica-<v>-linux-<arch>.tar.gz`, `.deb`, `.rpm`, `.AppImage`, `.AppImage.zsync`, `.flatpak` (six files per architecture) | x86_64 / aarch64: tar 18 / 16 MB, deb 18 / 17 MB, rpm 18 / 17 MB, AppImage 17 / 15 MB, zsync 59 / 53 KB, flatpak 12 / 9.9 MB | M6.21 |
| Windows x64 | `dalica-<v>-windows-x64.msi`, `dalica-<v>-windows-x64-portable.zip` (both `.exe`, the `portable` marker, the licences) | MSI 12.7 MiB, zip 15.8 MiB | M6.22 |
| macOS universal | `dalica-<v>-macos-universal.dmg`, `dalica-cli-<v>-macos-universal.zip` | DMG 20.2 MB, CLI zip 12.6 MB | M6.23 |

Each platform script checks its own artifacts before the job uploads them (*What gets built*
below says what each checks) and prints what it made with the sizes; the Windows and macOS
scripts also write their sizes and checks to the job summary.

Before the first push to `release`, a maintainer creates the **`release` environment** (Settings →
Environments) with its deployment branches limited to `release`. GitHub creates a missing
environment on first use *without* that limit, so the order matters.

## What gets built

| Platform | Artifacts | Notes |
|---|---|---|
| Linux x86_64, aarch64 | `dalica-<v>-linux-<arch>.{AppImage,AppImage.zsync,deb,rpm,tar.gz,flatpak}` | built on the oldest supported Ubuntu LTS runner for a low glibc floor |
| Windows x64 (ARM64 later) | `dalica-<v>-windows-<arch>.msi`, `dalica-<v>-windows-<arch>-portable.zip` | static C runtime, no redistributable needed |
| macOS 11+ universal | `dalica-<v>-macos-universal.dmg`, `dalica-cli-<v>-macos-universal.zip` | Apple silicon + Intel joined with `lipo` |

A web build and other platforms (FreeBSD) are out of scope until the maintainer decides otherwise.

Every binary reports its version, commit and build date: `dalica --version`,
`dalica-cli --version`, and Help ▸ About Dalica. CI sets `DALICA_BUILD_SHA` and
`DALICA_BUILD_DATE`; a plain `cargo build` reports `<version> (dev build)`.

Release builds check out [craft-fonts](https://github.com/storytold/craft-fonts) at a pinned commit
and build with `CRAFT_FONTS_DIR` and `CRAFT_FONTS_REQUIRED=1`: `crates/ui-kit/build.rs` embeds every
face of its manifest, and fails the build, naming what is missing, when the directory, a face or a
required UI face is absent (BIZ UDPGothic Regular for Japanese, Noto Sans CJK SC Regular for
Chinese, Noto Sans Thai Regular for Thai), so a release never ships without the fallback faces
([`assets.md`](assets.md) §4). Packages carry each embedded family's licence as
`OFL-<family>.txt`, copied from the checkout's `fonts/*/OFL.txt`; `dalica_ui_kit::fonts::licences()`
lists the families and licence files the binary embeds. Bump the pin deliberately, whenever a
shipped language needs a face the old pin lacks: a stale pin still builds, it just shows boxes
instead of glyphs. The pin is `CRAFT_FONTS_REF` at the top of `release.yml` and again at the top
of `ci.yml`, whose review-gate jobs (tests and accept, on every system) build with the fonts and
`CRAFT_FONTS_REQUIRED=1`, so `crates/ui-kit/tests/fonts.rs` checks every glyph of the Japanese,
Chinese, Thai and other manifest scripts' samples there; bump both together. The pin
(`3d9ab3a`, 2026-10-19) carries the Japanese, Simplified Chinese, Arabic and Thai faces. Before a
release, run the glyph tests with `CRAFT_FONTS_DIR` set
(`CRAFT_FONTS_DIR=../craft-fonts cargo test -p dalica-ui-kit --test all fonts::`, and the i18n
coverage test of [`localization.md`](localization.md)).

Every package ships `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` and `ATTRIBUTION.md`.

### Linux

`packaging/linux/package.sh [--skip-build] [--formats "tar deb rpm appimage flatpak"]
[--install-tools] [--require-all]` stages one FHS tree (both binaries, `org.dalica.Dalica.desktop`,
hicolor icons from 16 px to 512 px plus a scalable SVG, AppStream metainfo, the licences) and makes
every format from it. The desktop entry runs `dalica %f` (a file alone opens the photo viewer on
it, `dalica --view FILE`) and declares in `MimeType` the types Dalica can be the default viewer for
(`viewing.md` §3.3; not HEIC/HEIF while release builds leave out `heif`, nor AVIF), so the desktop
offers it under Open with and Options ▸ Photo Viewer ▸ File Types can make it the default:

- **AppImage** runs on any distribution without installing anything; built with the maintained
  `appimagetool` (static type2 runtime, no libfuse2). Each embeds `gh-releases-zsync` update
  information and ships a `.zsync`, so AppImageUpdate downloads only changed blocks. Its `AppRun`
  installs a user-level `.desktop` file on launch (best effort; `DALICA_NO_DESKTOP_INTEGRATION=1`
  opts out) so Wayland docks show the right icon.
- **.deb** and **.rpm**, both built by [nfpm](https://nfpm.goreleaser.com/) from one config
  (`packaging/linux/nfpm.yaml`), declare the runtime libraries (Wayland/X11, xkbcommon, Vulkan) and
  a glibc floor computed from the binaries, and integrate with the menu and icon caches.
- **.tar.gz** for people who manage their own `/opt` or `~/.local`.
- **Flatpak bundle** (`.flatpak`), made from the tarball on the freedesktop 26.08 runtime with plain
  `flatpak build-*` commands, so no SDK is downloaded and no Rust build runs inside Flatpak
  (`packaging/linux/flatpak/org.dalica.Dalica.bundle.yml` stays a valid `flatpak-builder`
  manifest). Sandbox: `wayland`, `fallback-x11`, `ipc`, `dri` and **read/write access to
  `xdg-pictures`**; no network and no home folder. The Folder Manager browses the file system
  itself (no portal file chooser yet), so a folder outside Pictures needs
  `flatpak override --user --filesystem=<folder> org.dalica.Dalica`. Map tiles and other online
  features stay off inside the sandbox until the user grants the network the same way. The
  from-source manifest for Flathub has the same runtime and permissions (below, *Flathub*).

Tools: nfpm 2.47.0, appimagetool 1.9.1 and the type2 runtime 20251108, each pinned with its
SHA-256 in `packaging/linux/tools.sh`, plus `zsyncmake`, `flatpak`, `desktop-file-validate` and
`appstreamcli`. Locally the script downloads nothing: a format whose tool is missing is skipped and
named in the summary. `--install-tools` installs the pinned tools and `--require-all` makes any
skipped format or check an error; both are on in GitHub Actions (`DALICA_INSTALL_TOOLS=0` turns
them off).

Checks, in `package.sh`: both binaries report the version and the commit; `dalica` refuses to start
on a planted system missing a library, naming its package; the desktop entry and the metainfo
validate; `AppRun`'s desktop integration (`apprun-test.sh`); every format holds exactly the staged
tree's files; the AppImage runs `--version` and carries its update information; the Flatpak's
sandbox is exactly the manifest's. With `--install-tools` also: the `.deb` installs, runs and
uninstalls cleanly, and the Flatpak installs into a private installation (its runtime from
Flathub) and runs.

Because the AppImage and tarball cannot declare dependencies, `dalica` looks where the loader looks
(`LD_LIBRARY_PATH`, `/etc/ld.so.cache`, `/etc/ld.so.conf` and its includes) for the libraries its
session loads before opening a window and, if one is missing, prints the package to install and
exits with status 1 instead of crashing. `DALICA_SKIP_LIB_CHECK=1` skips the check;
`DALICA_LIB_CHECK_ROOT=<dir>` reads the system files under another root (how `package.sh` tests the
refusal).

#### Flathub

`packaging/linux/flatpak/org.dalica.Dalica.yml` builds Dalica from source for Flathub: the
freedesktop 26.08 runtime and SDK with the `rust-stable` extension, the same `finish-args` as the
bundle (the Packaging lint workflow checks the two manifests agree on id, command, runtime, SDK and
permissions), `cargo --offline build --release --locked -p dalica -p dalica-cli` without the
network, then both binaries, the desktop entry, the metainfo (version from `Cargo.toml`, the date
from `DALICA_BUILD_DATE` or today), the hicolor icons and the licences (with the embedded fonts'
OFL) under `/app`. The craft-fonts families come from a git source at the release's pin
(`CRAFT_FONTS_REF` in `release.yml`), so the Flathub build embeds the same fonts as the release
packages.

The crates come from `cargo-sources.json`: one static.crates.io archive per `Cargo.lock` package
with its lock checksum, vendored into `cargo/vendor`, the layout flatpak-builder-tools'
`flatpak-cargo-generator` writes. `cargo xtask flatpak` writes it from `Cargo.lock` (our own Rust
generator: no Python, no network). It is generated, never committed (gitignored), because it would
go stale with every lock change; `cargo xtask flatpak --check`, in every gate, fails when the lock
holds a package the generator cannot vendor (a git or other registry dependency) or the manifest's
craft-fonts commit is not the release's pin. A local build:

```sh
cargo xtask flatpak
flatpak-builder --user --install --force-clean --install-deps-from=flathub \
  target/flatpak-build packaging/linux/flatpak/org.dalica.Dalica.yml
flatpak run org.dalica.Dalica
```

**Submitting a release.** `cargo xtask flatpak --flathub v<version> [--out <dir>]` (default
`target/flathub`) writes the submission files from the tag's own `Cargo.lock` and manifest:
`org.dalica.Dalica.yml` with the local `dir` source replaced by a git source pinned to the tag and
its commit (and that commit and its date as `DALICA_BUILD_SHA` and `DALICA_BUILD_DATE`, so
`--version` names it), and `cargo-sources.json`. The first submission is a pull request to
`flathub/flathub` (branch `new-pr`) with those two files; each later release is a pull request to
the app's own `flathub/org.dalica.Dalica` repository with them regenerated. Both are the
maintainer's to open. Before the first one, the maintainer decides:

- **The application id's domain.** Flathub verifies that the publisher controls the domain an id is
  built from (`dalica.org` for `org.dalica.Dalica`). Without it the id would change to one under a
  domain the project controls (for example `io.github.pkkid.Dalica`), which renames the desktop
  entry, the metainfo, the icons and `crates/core/src/app_id.rs`, everywhere at once.
- **Screenshots.** Flathub's linter requires `<screenshots>` in the metainfo, with image URLs that
  stay up (for example from the website); the template has none yet.
- **Release notes**, optional: a `<description>` per `<release>` shows on the Flathub page.
- **The network.** Neither manifest grants it, so map tiles and other online features stay off
  until the user runs `flatpak override`; granting `--share=network` instead is one decision for
  both manifests.

### Windows

`packaging/windows/package.ps1 -Arch x64 [-SkipBuild] [-Test | -NoTest] [-InstallTools]` builds with
`-C target-feature=+crt-static` and makes the MSI and the portable zip. It checks both programs' PE
machine and subsystem (`dalica.exe` GUI, `dalica-cli.exe` console), their `--version`, and
`dalica.exe`'s version resource and embedded icon. The app's `build.rs` embeds the icon and
VERSIONINFO (only when targeting Windows; a missing resource compiler returns an error naming the
tool, never a panic; `RC_PATH` (MSVC) or `WINDRES` (GNU) names it explicitly). Release builds use
the GUI subsystem, so `dalica.exe --version` typed in a console prints nothing; `dalica-cli` stays a
console program.

- **MSI** (WiX v5, `dalica.wxs` + `dalica.en-us.wxl`): per-machine install with a Start Menu
  shortcut and a setup wizard without a licence page (`msiexec /qn` still installs silently); the
  licence and notice files are installed beside the programs. Dalica is listed under "Open with"
  for the image formats it reads (not HEIC/HEIF while release builds leave out `heif`, nor AVIF)
  but never takes over the default photo viewer; Open with runs `dalica.exe --view "%1"`, which
  opens the photo viewer. Its Capabilities (`HKLM\Software\Dalica\Capabilities`, the same
  extensions) and `RegisteredApplications` entry list Dalica in Settings ▸ Default apps, where
  only the user may choose it (Options ▸ Photo Viewer ▸ File Types opens that page; a portable
  copy registers the same for the user, `viewing.md` §3.3). The MSI version is numeric `X.Y.Z`;
  same-version upgrades are allowed so release candidates replace each other.
- **Portable zip** ships a marker file named `portable` beside `dalica.exe` (a text file
  explaining itself; its contents are ignored) that keeps the library, thumbnail cache and log in
  `data\` next to the exe instead of `%LOCALAPPDATA%`. Sidecars still live in the photo folders.
- **Signing** (M15.10) goes through one script (`packaging/windows/sign.ps1`) that signs both
  `.exe` files and the `.msi` with SHA-256 and an RFC 3161 timestamp (three tries for a busy
  timestamp server), using either a `.pfx` certificate or Azure Trusted Signing, whichever is
  configured. The build job runs `package.ps1 -NoSign` and uploads its stage; the
  `sign Windows x64` job runs `package.ps1 -FromStage`, which signs the programs, builds the MSI
  around them, signs it and zips the portable set again, and `test-sign.ps1` checks every
  signature (valid, the expected certificate, a timestamp, the programs inside the MSI). Without
  signing material each unsigned file is named in a warning. A dry run signs with
  `sign.ps1 -Throwaway`: a self-signed certificate valid for a day, trusted on that runner only.

Tools: Rust with the MSVC target and the Windows SDK (`rc.exe`, `signtool.exe`), and WiX 5.0.2 with
its UI and Util extensions. In CI (or with `-InstallTools`) the script installs WiX itself;
elsewhere a missing WiX skips the MSI with a warning.

Tests (in CI, or with `-Test`; `-NoTest` skips them), with their results and the sizes in the job
summary: `test-build-rs.ps1` (a copy of `build.rs` with and without a resource compiler: the
missing tool is named; the release exe carries the icon and version and is GUI, a debug build stays
console), `test-portable.ps1` (the unpacked zip keeps library, cache and log in `data\`, and
without the marker uses `%LOCALAPPDATA%`), `test-msi.ps1` (silent install, the installed version
resource and `--version`, Open with for `.jpg` and `.cr3` with the defaults unchanged, a
same-version upgrade, a clean uninstall). `pe.ps1` holds the PE header checks.

### macOS

`packaging/macos/package.sh [--skip-build]` builds `aarch64-apple-darwin` and `x86_64-apple-darwin`
with `MACOSX_DEPLOYMENT_TARGET=11.0`, joins them with `lipo`, and assembles `Dalica.app`
(`Info.plist` from `Info.plist.in` with the application id as bundle id, image formats registered
with Dalica as their Viewer at Alternate rank, HEIC left out while release builds lack `heif`;
the user makes Dalica the default in Finder's Get Info ▸ Open with ▸ Change All…, which Options ▸
Photo Viewer ▸ File Types explains, `viewing.md` §3.3). Finder's Open With
sends the file as an Apple Event, which Dalica does not handle yet: the app starts and ignores it.

- **Signing** (M15.13) inside-out with the hardened runtime and a secure timestamp, no `--deep`:
  the executable, then `Dalica.app`, then the DMG, and `dalica-cli` (identifier
  `org.dalica.Dalica.cli`) before it is zipped. `entitlements.plist` is empty (pure Rust, no JIT,
  no unsigned code, not sandboxed) until a feature proves it needs one. Without
  `MACOS_SIGN_IDENTITY` everything is signed ad hoc, with a warning. In the release workflow the
  build job signs ad hoc and uploads its stage; the `sign macOS universal` job runs
  `package.sh --from-stage` on that stage. `import-cert.sh` puts the identity into a temporary
  keychain (`$RUNNER_TEMP/dalica-signing.keychain-db`, unlocked for six hours, on the search list,
  the key usable by `codesign` without a prompt) and exports `MACOS_SIGN_IDENTITY` (its SHA-1) and
  `MACOS_KEYCHAIN`; `import-cert.sh --delete`, the job's last step under `if: always()`, deletes
  the keychain.
- **The dry run's identity:** `import-cert.sh --throwaway` (GitHub Actions only) makes an RSA key
  and a self-signed code-signing certificate valid for one day, imports it the same way and trusts
  it for code signing on that runner (admin trust settings; `--delete` removes them when it can do so
  without a prompt, in bounded time), and sets
  `MACOS_SIGN_THROWAWAY=1`, so the dry run signs with the hardened runtime and Apple's timestamp
  exactly as a release does. A throwaway signature is never notarized: `package.sh` refuses
  notarization credentials beside it, and says what it skips by name (notarization of the app, the
  DMG and the CLI zip; the stapled tickets; the Developer ID and Gatekeeper checks).
- **Notarization** (`notarize.sh`, with a Developer ID identity and all three of `APPLE_ID`,
  `APPLE_PASSWORD`, `APPLE_TEAM_ID`; one or two of them is a warning, never a partial run):
  `xcrun notarytool submit --wait` for each file, Apple's log printed on a rejection; the app is
  zipped for the upload and notarized before the DMG is made, so the copy on the DMG carries its
  ticket; the ticket is stapled to the app and to the DMG (retried while it reaches Apple's CDN)
  and validated. The CLI zip is notarized too (a bare binary cannot hold a stapled ticket;
  Gatekeeper looks it up online).
- **DMG** built with `hdiutil create -srcfolder` (bounded retries), with an `Applications` link
  and a Finder window layout generated at every build by `packaging/macos/dmg/generate.py`
  (standard-library Python: the `.DS_Store` and a background drawn from the app icon, generated
  twice and compared, nothing binary committed). The volume is named `Dalica` without the version,
  because the window's background alias resolves by volume name; the DMG file name keeps the
  version.
- **`verify.sh`**, run last, checks the artifacts as users download them: mounts the DMG
  read-only and checks the volume name, the `Applications` link, the layout against a fresh
  generator run, both architectures, `Info.plist`, strict signatures with the hardened runtime
  (also after a `ditto` copy as an install makes) and runs the app's `--version`; the CLI zip
  likewise. What the signatures must show follows the signing mode: with any real identity the
  expected certificate (its SHA-1) and a secure timestamp on the app, the CLI and the DMG; with a
  Developer ID also its authority and team; with the notarization credentials also
  `stapler validate` on the app and the DMG and Gatekeeper's `spctl --assess` on all three. With
  the throwaway identity the notarization checks are skipped and named; ad hoc, the certificate
  checks are warnings, which is enough to check the bundle locally. The result goes to the job
  summary.
- **`test.sh`** runs the template and layout checks on any OS in seconds (`package.sh` runs it
  first).

## Secrets

All secrets live in the repository's **`release` environment**, whose deployment branches are
restricted to `release`. Only the signing jobs declare `environment: release`; the draft job runs
without it and cannot be reached from another branch, because it needs the signing jobs. **Every
secret is optional:** if one is missing, that platform's artifacts are unsigned and the run shows a
`::warning::`. Configure only what you need.

| Secret | Used for |
|---|---|
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` | base64 Developer ID Application `.p12` (identity and private key) and its password |
| `KEYCHAIN_PASSWORD` | temporary CI keychain (random if unset) |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | notarization: the Apple ID, an app-specific password for it, the team id of the certificate; all three or none |
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | base64 `.pfx` code-signing certificate and its password |
| `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE` | Azure Trusted Signing, an alternative to the `.pfx` |

Dry runs never read any of them: the signing jobs pass each secret as
`real && secrets.X || ''` and make their own throwaway identities. What a release run's secrets
produce is checked first on the 1.0 release run (M15.R): the Windows signatures name the real
certificate, the macOS ones the Developer ID with the team, and `verify.sh` passes strict with the
stapled tickets and Gatekeeper's assessment.

`GITHUB_TOKEN` creates the release; only the final job gets `contents: write`. Linux artifacts are
not code-signed: the release job computes SHA-256 checksums for the complete artifact set and
attaches `SHA256SUMS.txt` to the draft. Users verify downloads against it.

## Icons

`assets/app-icon/` holds the canonical vector icon and every derived PNG/ICNS/ICO/hicolor file,
all committed so packaging never needs the render tools; `packaging/icons.sh` regenerates them.
Rules for the icon itself are in [`assets.md`](assets.md) → App icon.

## Checks

The `Packaging lint` workflow (`.github/workflows/packaging-lint.yml`, since M6.13) runs in seconds
on any change to `packaging/`, the workflows or the icons: actionlint (its download checked against
a pinned SHA-256; it also runs shellcheck over the workflows' scripts), shellcheck over every
`packaging/**/*.sh`, the checksum script's planted-mismatch test, a PowerShell parse of every
`packaging/**/*.ps1`, and xmllint, `desktop-file-validate`, `appstreamcli validate` and a check that
the Flatpak manifests agree on id, command, runtime and permissions. The last four find their files
by pattern (the WiX sources, the macOS property lists, the Linux desktop entry and metainfo, the
Flatpak manifest).
