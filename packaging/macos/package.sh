#!/usr/bin/env bash
# Build, sign and (with the secrets) notarize Dalica for macOS 11 and later, universal (Apple
# silicon + Intel), then check the result as users get it:
#
#   $DIST/dalica-<version>-macos-universal.dmg        Dalica.app on a drag-to-Applications DMG
#   $DIST/dalica-cli-<version>-macos-universal.zip    dalica-cli
#
# Usage: packaging/macos/package.sh [--skip-build]
#
# Steps: both architectures built at MACOSX_DEPLOYMENT_TARGET=11.0 and joined with lipo; the DMG
# window layout generated twice and compared (dmg/generate.py, standard-library Python); Dalica.app
# assembled (Info.plist.in, the icon, the licences) and signed inside-out with the hardened runtime;
# the DMG (volume "Dalica", an Applications link, the layout) and the CLI zip; then verify.sh, which
# mounts the DMG and runs the app's --version.
#
# Signing (env): MACOS_SIGN_IDENTITY (codesign identity, name or SHA-1; import-cert.sh sets it in
# CI), MACOS_KEYCHAIN (optional). Without an identity everything is signed ad hoc, with a warning:
# enough to check the bundle, refused by Gatekeeper on other Macs.
# Notarization (env, all three and a real identity): APPLE_ID, APPLE_PASSWORD (app-specific
# password), APPLE_TEAM_ID. Then the app and the DMG get stapled tickets; the CLI zip is notarized
# too (a bare binary cannot hold a ticket; Gatekeeper looks it up online).
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
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,32p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
APP="$WORK/Dalica.app"

NOTARIZE=0
if [ "$IDENTITY" = "-" ]; then
  warn "macOS: ad hoc signing (no MACOS_SIGN_IDENTITY): the artifacts are not notarized and Gatekeeper refuses them on other Macs"
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  NOTARIZE=1
else
  warn "macOS: APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID are not all set: signed but not notarized"
fi

mkdir -p "$DIST"
echo "==> Dalica $VERSION for macOS universal (${DALICA_BUILD_SHA:0:9}, $DALICA_BUILD_DATE), identity: $IDENTITY, notarize: $NOTARIZE"

# ---- build -------------------------------------------------------------------------------------
if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p dalica -p dalica-cli --target "${TARGETS[0]}" --target "${TARGETS[1]}")
fi

rm -rf "$WORK"
mkdir -p "$WORK/bin"
for exe in dalica dalica-cli; do
  lipo -create -output "$WORK/bin/$exe" \
    "$TARGET_DIR/${TARGETS[0]}/release/$exe" "$TARGET_DIR/${TARGETS[1]}/release/$exe"
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
  # Hardened runtime and a secure timestamp for a real identity; ad hoc cannot be timestamped.
  local ts=(--timestamp) kc=()
  if [ "$IDENTITY" = "-" ]; then ts=(--timestamp=none); fi
  if [ -n "${MACOS_KEYCHAIN:-}" ]; then kc=(--keychain "$MACOS_KEYCHAIN"); fi
  codesign --force --sign "$IDENTITY" ${kc[@]+"${kc[@]}"} "${ts[@]}" "$@"
}

notarize() {
  local file="$1" out id status
  echo "==> notarizing $(basename "$file") (this can take a few minutes)"
  out="$(xcrun notarytool submit "$file" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" --wait --timeout 1h --output-format json)" || true
  echo "$out"
  id="$(printf '%s' "$out" | plutil -extract id raw -o - - 2>/dev/null || true)"
  status="$(printf '%s' "$out" | plutil -extract status raw -o - - 2>/dev/null || true)"
  if [ "$status" != "Accepted" ]; then
    if [ -n "$id" ]; then
      xcrun notarytool log "$id" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" || true
    fi
    echo "error: notarization of $(basename "$file") failed (status: ${status:-unknown})" >&2
    exit 1
  fi
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
copy_licences "$APP/Contents/Resources/Licenses"
# No extended attributes from the build may reach the signature.
xattr -cr "$APP"

# Inside-out: nested code first, then the bundle (no --deep when signing). Today the only nested
# code is the main executable; frameworks or helpers would be signed here first.
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP/Contents/MacOS/Dalica"
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP"
codesign --verify --strict --deep --verbose=2 "$APP"

if [ "$NOTARIZE" = 1 ]; then
  ditto -c -k --keepParent "$APP" "$WORK/Dalica-notarize.zip"
  notarize "$WORK/Dalica-notarize.zip"
  xcrun stapler staple "$APP"
  xcrun stapler validate "$APP"
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
if [ "$NOTARIZE" = 1 ]; then
  notarize "$DMG"
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"
fi

# ---- CLI ---------------------------------------------------------------------------------------
echo "==> building $CLI_ZIP"
CLI_DIR="$WORK/$CLI_DIR_NAME"
mkdir -p "$CLI_DIR"
cp "$WORK/bin/dalica-cli" "$CLI_DIR/"
copy_licences "$CLI_DIR"
# The app's hardened runtime and entitlements, with a reverse-DNS identifier (codesign would
# otherwise use the bare file name).
sign --options runtime --entitlements "$HERE/entitlements.plist" --identifier "$APP_ID.cli" "$CLI_DIR/dalica-cli"
codesign --verify --strict --verbose=2 "$CLI_DIR/dalica-cli"
rm -f "$CLI_ZIP"
# ditto keeps the executable bit and makes the zip Finder's Archive Utility expects.
ditto -c -k --keepParent "$CLI_DIR" "$CLI_ZIP"
if [ "$NOTARIZE" = 1 ]; then notarize "$CLI_ZIP"; fi

# ---- check what users download -----------------------------------------------------------------
"$HERE/verify.sh"
echo "==> done"
ls -l "$DMG" "$CLI_ZIP"
