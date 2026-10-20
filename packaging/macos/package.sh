#!/usr/bin/env bash
# Build, sign and (with the secrets) notarize Dalica for macOS 11 and later, universal (Apple
# silicon + Intel), then check the result as users get it:
#
#   $DIST/dalica-<version>-macos-universal.dmg        Dalica.app on a drag-to-Applications DMG
#   $DIST/dalica-cli-<version>-macos-universal.zip    dalica-cli
#
# Usage: packaging/macos/package.sh [--skip-build | --from-stage DIR]
#
# Steps: both architectures built at MACOSX_DEPLOYMENT_TARGET=11.0 and joined with lipo, and the
# stage written ($WORK/bin: the universal dalica and dalica-cli; $WORK/doc: the licence files); the
# DMG window layout generated twice and compared (dmg/generate.py, standard-library Python);
# Dalica.app assembled (Info.plist.in, the icon, the licences) and signed inside-out with the
# hardened runtime; the DMG (volume "Dalica", an Applications link, the layout) and the CLI zip;
# then verify.sh, which mounts the DMG and runs the app's --version.
#
# --from-stage DIR takes a build job's stage (DIR/bin, DIR/doc) instead of building: the release
# workflow's signing job, which has no Rust toolchain and alone sees the signing secrets.
#
# Signing (env): MACOS_SIGN_IDENTITY (codesign identity, name or SHA-1; import-cert.sh sets it in
# CI), MACOS_KEYCHAIN (optional), MACOS_SIGN_THROWAWAY=1 for the dry run's self-signed identity
# (import-cert.sh --throwaway). Without an identity everything is signed ad hoc, with a warning:
# enough to check the bundle, refused by Gatekeeper on other Macs.
# Notarization (env, all three and a Developer ID identity): APPLE_ID, APPLE_PASSWORD (app-specific
# password), APPLE_TEAM_ID; notarize.sh submits the app, the DMG and the CLI zip and staples the
# app and the DMG (a bare binary cannot hold a ticket; Gatekeeper looks the CLI's up online). A
# throwaway identity is never notarized, and the run names what it skipped.
# Other environment (all optional): DALICA_VERSION, DALICA_BUILD_SHA, DALICA_BUILD_DATE, DIST,
# CARGO_TARGET_DIR, CRAFT_FONTS_DIR (common.sh).
#
# Needs: Xcode command line tools (lipo, codesign, hdiutil, ditto, plutil), python3 (3.8 or later,
# standard library only) and the Rust targets aarch64-apple-darwin and x86_64-apple-darwin.
#
# Adapted from LightCraft packaging/macos/package.sh @ 9978dc73 and PhotoCraft
# packaging/macos/package.sh @ 77221725 (MIT OR Apache-2.0): the lipo build, the signing and
# notarization helpers, the bundle assembly, `hdiutil create -srcfolder` with bounded retries and
# the CLI zip. Changed for Dalica: the DMG layout is generated at build time and checked for
# determinism, and every check of the finished artifacts lives in verify.sh, run at the end.
set -euo pipefail
# shellcheck source=common.sh
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

SKIP_BUILD=0
FROM_STAGE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --from-stage)
      [ $# -ge 2 ] || { echo "--from-stage needs a folder" >&2; exit 2; }
      FROM_STAGE="$(cd "$2" && pwd)"; SKIP_BUILD=1; shift 2 ;;
    -h | --help) sed -n '2,33p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
APP="$WORK/Dalica.app"

# IDENTITY, SIGN_MODE, NOTARY_SET and NOTARIZE come from common.sh.
case "$SIGN_MODE" in
  adhoc)
    warn "macOS: ad hoc signing (no MACOS_SIGN_IDENTITY): the artifacts are not notarized and Gatekeeper refuses them on other Macs" ;;
  throwaway)
    if [ "$NOTARY_SET" != 0 ]; then
      echo "error: notarization credentials are set with the throwaway identity; a dry run never reads them" >&2
      exit 1
    fi
    echo "Dry run: signing with the throwaway identity $IDENTITY; skipped: $NOT_NOTARIZED" ;;
  developer-id)
    case "$NOTARY_SET" in
      3) ;;
      0) warn "macOS: APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID are not set: signed but not notarized" ;;
      *) warn "macOS: only $NOTARY_SET of APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID are set (all three or none): signed but not notarized" ;;
    esac ;;
esac

mkdir -p "$DIST"
echo "==> Dalica $VERSION for macOS universal (${DALICA_BUILD_SHA:0:9}, $DALICA_BUILD_DATE), identity: $IDENTITY ($SIGN_MODE), notarize: $NOTARIZE"

# ---- build -------------------------------------------------------------------------------------
if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p dalica -p dalica-cli --target "${TARGETS[0]}" --target "${TARGETS[1]}")
fi

rm -rf "$WORK"
mkdir -p "$WORK/bin"
if [ -n "$FROM_STAGE" ]; then
  echo "==> the stage $FROM_STAGE"
  for f in bin/dalica bin/dalica-cli doc/LICENSE-MIT; do
    [ -f "$FROM_STAGE/$f" ] || { echo "error: the stage has no $f" >&2; exit 1; }
  done
  cp -R "$FROM_STAGE/doc" "$WORK/doc"
else
  copy_licences "$WORK/doc"
fi
for exe in dalica dalica-cli; do
  if [ -n "$FROM_STAGE" ]; then
    # Workflow artifacts do not keep the executable bit.
    cp "$FROM_STAGE/bin/$exe" "$WORK/bin/$exe"
    chmod 755 "$WORK/bin/$exe"
  else
    lipo -create -output "$WORK/bin/$exe" \
      "$TARGET_DIR/${TARGETS[0]}/release/$exe" "$TARGET_DIR/${TARGETS[1]}/release/$exe"
  fi
  lipo "$WORK/bin/$exe" -verify_arch arm64 x86_64
  lipo -info "$WORK/bin/$exe"
  # It must report this version and the commit, not a dev build.
  out="$("$WORK/bin/$exe" --version)"
  echo "$exe --version: $out"
  case "$out" in *"$VERSION ("*) ;; *) echo "error: $exe reports '$out', not $VERSION" >&2; exit 1 ;; esac
  if [ -n "$DALICA_BUILD_SHA" ]; then
    case "$out" in *"${DALICA_BUILD_SHA:0:9}"*) ;; *) echo "error: $exe does not report commit ${DALICA_BUILD_SHA:0:9}" >&2; exit 1 ;; esac
  fi
done

# ---- the DMG window layout ---------------------------------------------------------------------
# test.sh first: the generator's self-test (it reproduces the Craft apps' committed layout files)
# and the Info.plist template. Then the layout for this DMG, generated twice: the two runs must
# agree byte for byte.
"$HERE/test.sh"
echo "==> DMG layout (dmg/generate.py)"
python3 "$HERE/dmg/generate.py" "$WORK/layout"
python3 "$HERE/dmg/generate.py" "$WORK/layout-again" >/dev/null
for f in background.tiff DS_Store; do
  if ! cmp "$WORK/layout/$f" "$WORK/layout-again/$f"; then
    echo "error: two runs of dmg/generate.py wrote different $f" >&2
    exit 1
  fi
done
echo "ok: the layout files are identical across two runs"

# ---- signing helpers ---------------------------------------------------------------------------
sign() {
  # Hardened runtime and a secure timestamp for a real identity (the throwaway too); ad hoc cannot
  # be timestamped. Apple's timestamp server can be briefly unreachable: bounded retries.
  local ts=(--timestamp) kc=() attempt
  if [ "$SIGN_MODE" = adhoc ]; then ts=(--timestamp=none); fi
  if [ -n "${MACOS_KEYCHAIN:-}" ]; then kc=(--keychain "$MACOS_KEYCHAIN"); fi
  for attempt in 1 2 3; do
    if codesign --force --sign "$IDENTITY" ${kc[@]+"${kc[@]}"} "${ts[@]}" "$@"; then return 0; fi
    if [ "$attempt" = 3 ]; then
      echo "error: codesign failed three times: ${*: -1}" >&2
      return 1
    fi
    warn "codesign attempt $attempt failed; retrying"
    sleep 10
  done
}

# hdiutil can fail transiently with "Resource busy" on CI runners: bounded retries.
hdiutil_retry() {
  local attempt
  for attempt in 1 2 3 4 5; do
    if hdiutil "$@"; then return 0; fi
    if [ "$attempt" = 5 ]; then
      echo "error: hdiutil $1 failed after five attempts" >&2
      return 1
    fi
    warn "hdiutil $1 attempt $attempt failed; retrying"
    sleep $((attempt * 10))
  done
}

# ---- Dalica.app --------------------------------------------------------------------------------
echo "==> assembling $APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
# The executable and the icon carry the display name (CFBundleExecutable, CFBundleIconFile).
cp "$WORK/bin/dalica" "$APP/Contents/MacOS/Dalica"
cp "$ROOT/assets/app-icon/dalica.icns" "$APP/Contents/Resources/Dalica.icns"
render_info_plist "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"
cp -R "$WORK/doc" "$APP/Contents/Resources/Licenses"
# No extended attributes from the build may reach the signature.
xattr -cr "$APP"

# Inside-out: nested code first, then the bundle (no --deep when signing). Today the only nested
# code is the main executable; frameworks or helpers would be signed here first.
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP/Contents/MacOS/Dalica"
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP"
codesign --verify --strict --deep --verbose=2 "$APP"

if [ "$NOTARIZE" = 1 ]; then
  # Before the DMG is made, so the copy on it carries the stapled ticket.
  "$HERE/notarize.sh" "$APP"
  spctl --assess --type execute -vvv "$APP"
fi

# ---- DMG ---------------------------------------------------------------------------------------
echo "==> building $DMG"
STAGE="$WORK/dmg"
mkdir -p "$STAGE/.background"
ditto "$APP" "$STAGE/Dalica.app"
ln -s /Applications "$STAGE/Applications"
cp "$WORK/layout/background.tiff" "$STAGE/.background/background.tiff"
cp "$WORK/layout/DS_Store" "$STAGE/.DS_Store"
rm -f "$DMG"
# `create -srcfolder` keeps the signed app as it is (makehybrid adds FinderInfo to it, which breaks
# the strict signature check) and copies the .DS_Store. The volume name has no version: the
# layout's background alias finds the image by volume name (dmg/README.md).
hdiutil_retry create -srcfolder "$STAGE" -volname "Dalica" -fs HFS+ -format UDZO \
  -imagekey zlib-level=9 -ov "$DMG"
sign "$DMG"
codesign --verify --strict --verbose=2 "$DMG"
if [ "$NOTARIZE" = 1 ]; then "$HERE/notarize.sh" "$DMG"; fi

# ---- CLI ---------------------------------------------------------------------------------------
echo "==> building $CLI_ZIP"
CLI_DIR="$WORK/$CLI_DIR_NAME"
mkdir -p "$CLI_DIR"
cp "$WORK/bin/dalica-cli" "$CLI_DIR/"
cp -R "$WORK/doc/." "$CLI_DIR/"
# The app's hardened runtime and entitlements, with a reverse-DNS identifier (codesign would
# otherwise use the bare file name).
sign --options runtime --entitlements "$HERE/entitlements.plist" --identifier "$APP_ID.cli" "$CLI_DIR/dalica-cli"
codesign --verify --strict --verbose=2 "$CLI_DIR/dalica-cli"
rm -f "$CLI_ZIP"
# ditto keeps the executable bit and makes the zip Finder's Archive Utility expects.
ditto -c -k --keepParent "$CLI_DIR" "$CLI_ZIP"
if [ "$NOTARIZE" = 1 ]; then "$HERE/notarize.sh" "$CLI_ZIP"; fi

# ---- check what users download -----------------------------------------------------------------
"$HERE/verify.sh"
echo "==> done"
ls -l "$DMG" "$CLI_ZIP"
