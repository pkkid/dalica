#!/usr/bin/env bash
# Check the macOS release artifacts exactly as users download them:
#
#   $DIST/dalica-<version>-macos-universal.dmg       mounted read-only: volume "Dalica", Dalica.app
#                                                    (both architectures, bundle id and version,
#                                                    strict signature, also after a ditto copy as
#                                                    an install makes; runs --version), the
#                                                    Applications link, and the window layout equal
#                                                    to a fresh dmg/generate.py run
#   $DIST/dalica-cli-<version>-macos-universal.zip   unpacked: both architectures, strict
#                                                    signature, runs --version
#
# Usage: packaging/macos/verify.sh       (package.sh runs it last)
#
# What the signatures must show follows common.sh's signing mode (the MACOS_SIGN_IDENTITY,
# MACOS_SIGN_THROWAWAY, APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID that package.sh uses):
#
#   every mode     strict integrity (codesign --verify --strict) and the hardened runtime
#   throwaway      also: signed by that identity's certificate (its SHA-1), a secure timestamp, the
#                  DMG signed too; the notarization checks are skipped and named (a dry run)
#   developer-id   also: a Developer ID Application authority, the expected certificate when the
#                  identity is a SHA-1, the team when APPLE_TEAM_ID is set; with all three
#                  notarization credentials also the stapled tickets on the app and the DMG and
#                  Gatekeeper's assessment (for the CLI, an online lookup: a bare binary cannot hold
#                  a ticket)
#   adhoc          the certificate and notarization checks are warnings, enough to check a bundle
#                  locally
#
# Nothing here reads or prints a secret's value; it only tests whether one is set.
#
# Adapted from PhotoCraft packaging/macos/verify.sh @ 77221725 and the finished-image check of
# LightCraft packaging/macos/package.sh @ 9978dc73 (MIT OR Apache-2.0).
set -euo pipefail
# shellcheck source=common.sh
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

case "${1:-}" in
  "") ;;
  -h | --help) sed -n '2,30p' "$0"; exit 0 ;;
  *) echo "unknown argument: $1" >&2; exit 2 ;;
esac

# IDENTITY, SIGN_MODE and NOTARIZE come from common.sh. STRICT: the notarization checks apply.
STRICT="$NOTARIZE"
# The expected leaf certificate, when the identity is given by its SHA-1 (import-cert.sh does).
WANT_SHA1=""
re='^[0-9A-Fa-f]{40}$'
if [[ $IDENTITY =~ $re ]]; then WANT_SHA1="$(printf '%s' "$IDENTITY" | tr '[:lower:]' '[:upper:]')"; fi

FAILED=0
# A check that only a build signed with a real certificate can pass: an error unless ad hoc.
fail_signed() {
  if [ "$SIGN_MODE" != adhoc ]; then
    echo "error: $*" >&2
    FAILED=1
  else
    warn "$* (expected without signing secrets)"
  fi
}
# A check that only a notarized build can pass.
fail_notarized() {
  echo "error: $*" >&2
  FAILED=1
}
die() {
  echo "error: $*" >&2
  exit 1
}

# spctl's online ticket lookup can lag a few seconds behind notarytool's "Accepted".
assess() {
  local out attempt tries=1
  if [ "$STRICT" = 1 ]; then tries=5; fi
  for attempt in $(seq "$tries"); do
    if out="$(spctl "$@" 2>&1)"; then
      printf '%s\n' "$out"
      return 0
    fi
    if [ "$attempt" != "$tries" ]; then sleep 10; fi
  done
  printf '%s\n' "$out"
  return 1
}

# has_archs FILE NAME: FILE holds both slices.
has_archs() {
  local archs a
  archs="$(lipo -archs "$1")"
  for a in arm64 x86_64; do
    case " $archs " in *" $a "*) ;; *) die "$2 lacks $a (has: $archs)" ;; esac
  done
  lipo -info "$1"
}

# reports_version EXE NAME: --version names this version and commit.
reports_version() {
  local out
  out="$("$1" --version)" || die "$2 --version failed"
  echo "$2 --version: $out"
  case "$out" in *"$VERSION ("*) ;; *) die "$2 reports '$out', not $VERSION" ;; esac
  if [ -n "$DALICA_BUILD_SHA" ]; then
    case "$out" in *"${DALICA_BUILD_SHA:0:9}"*) ;; *) die "$2 does not report commit ${DALICA_BUILD_SHA:0:9}" ;; esac
  fi
}

# signer FILE NAME: the certificate, timestamp and team checks; prints the signature's details.
signer() {
  local details leaf
  details="$(codesign -d --verbose=2 "$1" 2>&1)"
  printf '%s\n' "$details" | grep -E '^(Identifier|Format|CodeDirectory|Authority|Timestamp|TeamIdentifier)[= ]' || true
  if [ "$SIGN_MODE" = developer-id ] || [ "$SIGN_MODE" = adhoc ]; then
    printf '%s\n' "$details" | grep -q '^Authority=Developer ID Application:' ||
      fail_signed "$2 is not signed with a Developer ID Application identity"
  fi
  printf '%s\n' "$details" | grep -q '^Timestamp=' ||
    fail_signed "$2 has no secure timestamp"
  if [ -n "$WANT_SHA1" ]; then
    rm -f "$TMP"/cert*
    codesign -d --extract-certificates="$TMP/cert" "$1" >/dev/null 2>&1 || true
    if [ -f "$TMP/cert0" ]; then
      leaf="$(shasum -a 1 "$TMP/cert0" | awk '{ print toupper($1) }')"
    else
      leaf=none
    fi
    if [ "$leaf" = "$WANT_SHA1" ]; then
      echo "$2: signed by the expected certificate $WANT_SHA1"
    else
      fail_signed "$2 is signed by certificate $leaf, not $WANT_SHA1"
    fi
  fi
  if [ -n "${APPLE_TEAM_ID:-}" ] && [ "$SIGN_MODE" = developer-id ]; then
    printf '%s\n' "$details" | grep -qx "TeamIdentifier=$APPLE_TEAM_ID" ||
      fail_signed "$2 is signed by a different team"
  fi
}

# signed_details FILE NAME: code (an executable or the bundle): the hardened runtime, then signer.
signed_details() {
  codesign -d --verbose=2 "$1" 2>&1 | grep -Eq '^CodeDirectory .*flags=0x[0-9a-f]+\([^)]*runtime' ||
    die "$2 is not signed with the hardened runtime"
  signer "$1" "$2"
}

for f in "$DMG" "$CLI_ZIP"; do
  [ -f "$f" ] || die "missing $f"
done

TMP="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/dalica-verify.XXXXXX")"
MNT="$TMP/mnt"
MOUNTED=0
cleanup() {
  if [ "$MOUNTED" = 1 ]; then hdiutil detach "$MNT" -force >/dev/null 2>&1 || true; fi
  rm -rf "$TMP"
}
trap cleanup EXIT

# ---- CLI zip -----------------------------------------------------------------------------------
echo "==> $(basename "$CLI_ZIP")"
ditto -x -k "$CLI_ZIP" "$TMP/cli"
CLI="$TMP/cli/$CLI_DIR_NAME/dalica-cli"
[ -x "$CLI" ] || die "$(basename "$CLI_ZIP") has no executable $CLI_DIR_NAME/dalica-cli"
for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md OFL-Inter.txt; do
  [ -f "$TMP/cli/$CLI_DIR_NAME/$f" ] || die "$(basename "$CLI_ZIP") lacks $f"
done
has_archs "$CLI" dalica-cli
# Integrity first: this holds for ad hoc builds too.
codesign --verify --strict --verbose=2 "$CLI"
signed_details "$CLI" dalica-cli
if [ "$STRICT" = 1 ]; then
  if gk="$(assess --assess --type install -vv "$CLI")"; then
    printf '%s\n' "$gk"
    printf '%s\n' "$gk" | grep -q 'source=Notarized Developer ID' ||
      fail_notarized "Gatekeeper accepts dalica-cli but not as notarized"
  else
    printf '%s\n' "$gk"
    fail_notarized "Gatekeeper rejects dalica-cli (not notarized)"
  fi
fi
reports_version "$CLI" dalica-cli

# ---- DMG ---------------------------------------------------------------------------------------
echo "==> $(basename "$DMG")"
codesign --verify --strict --verbose=2 "$DMG"
if [ "$SIGN_MODE" != adhoc ]; then signer "$DMG" "$(basename "$DMG")"; fi
if [ "$STRICT" = 1 ]; then
  xcrun stapler validate "$DMG" || fail_notarized "no notarization ticket stapled to the DMG"
  assess --assess --type open --context context:primary-signature -vv "$DMG" ||
    fail_notarized "Gatekeeper rejects the DMG"
fi

mkdir -p "$MNT"
attached=0
for attempt in 1 2 3 4 5; do
  if hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$MNT" "$DMG" >/dev/null; then
    attached=1
    break
  fi
  warn "hdiutil attach attempt $attempt failed; retrying"
  sleep $((attempt * 10))
done
[ "$attached" = 1 ] || die "cannot mount $(basename "$DMG")"
MOUNTED=1

volume="$(diskutil info -plist "$MNT" | plutil -extract VolumeName raw -o - -)"
[ "$volume" = "Dalica" ] || die "the DMG's volume is '$volume', not 'Dalica' (the layout's background alias needs that name)"
echo "volume: $volume"
if [ ! -L "$MNT/Applications" ] || [ "$(readlink "$MNT/Applications")" != /Applications ]; then
  die "the DMG has no Applications link to /Applications"
fi

# The window layout is the one dmg/generate.py writes, byte for byte.
python3 "$HERE/dmg/generate.py" "$TMP/layout" >/dev/null
cmp "$MNT/.DS_Store" "$TMP/layout/DS_Store" || die "the DMG's .DS_Store is not dmg/generate.py's"
cmp "$MNT/.background/background.tiff" "$TMP/layout/background.tiff" ||
  die "the DMG's .background/background.tiff is not dmg/generate.py's"
echo "layout: .DS_Store and .background/background.tiff match dmg/generate.py"

APP="$MNT/Dalica.app"
EXE="$APP/Contents/MacOS/Dalica"
[ -x "$EXE" ] || die "the DMG has no executable Dalica.app/Contents/MacOS/Dalica"
[ -f "$APP/Contents/Resources/Dalica.icns" ] || die "Dalica.app has no Dalica.icns"
[ -f "$APP/Contents/Resources/Licenses/LICENSE-MIT" ] || die "Dalica.app has no Licenses folder"
plist="$APP/Contents/Info.plist"
plutil -lint "$plist"
for pair in "CFBundleIdentifier=$APP_ID" "CFBundleExecutable=Dalica" "CFBundleShortVersionString=$SHORT_VERSION" \
  "DalicaVersion=$VERSION" "LSMinimumSystemVersion=$MACOSX_DEPLOYMENT_TARGET"; do
  key="${pair%%=*}"
  got="$(plutil -extract "$key" raw -o - "$plist")"
  [ "$got" = "${pair#*=}" ] || die "Info.plist $key is '$got', expected '${pair#*=}'"
done
echo "Info.plist: $APP_ID $VERSION"
has_archs "$EXE" Dalica.app
# The bundle as it lies on the image, and a copy made the way an install makes it: both strict.
codesign --verify --deep --strict --verbose=2 "$APP"
ditto "$APP" "$TMP/installed/Dalica.app"
codesign --verify --deep --strict --verbose=2 "$TMP/installed/Dalica.app"
signed_details "$TMP/installed/Dalica.app" Dalica.app
if [ "$STRICT" = 1 ]; then
  xcrun stapler validate "$TMP/installed/Dalica.app" || fail_notarized "no notarization ticket stapled to Dalica.app"
  assess --assess --type execute -vv "$TMP/installed/Dalica.app" || fail_notarized "Gatekeeper rejects Dalica.app"
fi
reports_version "$EXE" "Dalica.app (mounted DMG)"

hdiutil detach "$MNT" >/dev/null
MOUNTED=0

if [ "$FAILED" != 0 ]; then
  die "the macOS artifacts failed verification"
fi
case "$SIGN_MODE:$STRICT" in
  developer-id:1) result="signed (Developer ID, hardened runtime, timestamp), notarized; app and DMG stapled" ;;
  developer-id:0) result="signed (Developer ID, hardened runtime, timestamp); not checked: $NOT_NOTARIZED (not notarized)" ;;
  throwaway:*) result="signed with the dry run's throwaway identity $IDENTITY (hardened runtime, timestamp; the app, the CLI and the DMG; strict after a ditto copy); skipped: $NOT_NOTARIZED" ;;
  *) result="ad hoc signatures intact; not checked: $NOT_NOTARIZED (no signing secrets)" ;;
esac
echo "==> verified: $result"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  echo "- macOS \`$(basename "$DMG")\`, \`$(basename "$CLI_ZIP")\`: $result" >>"$GITHUB_STEP_SUMMARY"
fi
