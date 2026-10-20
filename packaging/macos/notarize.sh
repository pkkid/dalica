#!/usr/bin/env bash
# Notarize macOS release files with Apple's notary service, then staple the ticket where a file can
# hold one:
#
#   Dalica.app   zipped (ditto) for the upload, then the ticket stapled to the bundle and validated
#   *.dmg        uploaded as it is, then stapled and validated
#   *.zip        uploaded as it is; nothing to staple (a bare binary cannot hold a ticket, so
#                Gatekeeper looks the CLI's ticket up online)
#
# Usage: packaging/macos/notarize.sh FILE...        (package.sh runs it for each file)
#
# Inputs (env): APPLE_ID, APPLE_PASSWORD (an app-specific password), APPLE_TEAM_ID, all three, and a
# Developer ID signing identity in MACOS_SIGN_IDENTITY (common.sh's NOTARIZE=1); anything else is
# refused before an upload. `xcrun notarytool submit --wait` waits up to an hour per file; a
# rejection prints Apple's log for that submission and exits 1. Stapling retries for a few minutes,
# because the ticket can reach Apple's CDN a little after the service says "Accepted". Nothing here
# prints a secret's value.
#
# Adapted from the notarize helper of PhotoCraft packaging/macos/package.sh @ 77221725 and
# LightCraft packaging/macos/package.sh @ 9978dc73 (MIT OR Apache-2.0). Changed for Dalica: its own
# script, one submission per file with the log on rejection, and stapling with bounded retries.
set -euo pipefail
# shellcheck source=common.sh
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

case "${1:-}" in
  -h | --help) sed -n '2,18p' "$0"; exit 0 ;;
  "") echo "usage: packaging/macos/notarize.sh FILE..." >&2; exit 2 ;;
esac

if [ "$NOTARIZE" != 1 ]; then
  echo "error: notarize.sh needs a Developer ID identity (MACOS_SIGN_IDENTITY, not ad hoc or the throwaway) and all three of APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID (sign mode: $SIGN_MODE, $NOTARY_SET of 3 set)" >&2
  exit 1
fi

TMP="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/dalica-notarize.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

# submit FILE: upload FILE and wait for the verdict; Apple's log on anything but "Accepted".
submit() {
  local file="$1" out id status
  echo "==> notarizing $(basename "$file") (this can take a few minutes)"
  out="$(xcrun notarytool submit "$file" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" --wait --timeout 1h --output-format json)" || true
  printf '%s\n' "$out"
  id="$(printf '%s' "$out" | plutil -extract id raw -o - - 2>/dev/null || true)"
  status="$(printf '%s' "$out" | plutil -extract status raw -o - - 2>/dev/null || true)"
  if [ "$status" != "Accepted" ]; then
    if [ -n "$id" ]; then
      xcrun notarytool log "$id" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" || true
    fi
    echo "error: notarization of $(basename "$file") failed (status: ${status:-unknown})" >&2
    exit 1
  fi
  echo "ok: $(basename "$file") notarized (submission ${id:-?})"
}

# staple FILE: attach the ticket, retrying while it propagates, then validate it.
staple() {
  local file="$1" attempt
  for attempt in 1 2 3 4 5 6; do
    if xcrun stapler staple "$file"; then
      xcrun stapler validate "$file"
      return 0
    fi
    if [ "$attempt" = 6 ]; then
      echo "error: stapling $(basename "$file") failed after six attempts" >&2
      exit 1
    fi
    warn "stapler staple $(basename "$file") attempt $attempt failed; retrying"
    sleep $((attempt * 10))
  done
}

for file in "$@"; do
  case "$file" in
    *.app | *.app/)
      [ -d "$file" ] || { echo "error: no bundle $file" >&2; exit 1; }
      file="${file%/}"
      zip="$TMP/$(basename "$file" .app)-notarize.zip"
      rm -f "$zip"
      ditto -c -k --keepParent "$file" "$zip"
      submit "$zip"
      staple "$file"
      ;;
    *.dmg)
      [ -f "$file" ] || { echo "error: no file $file" >&2; exit 1; }
      submit "$file"
      staple "$file"
      ;;
    *.zip)
      [ -f "$file" ] || { echo "error: no file $file" >&2; exit 1; }
      submit "$file"
      ;;
    *)
      echo "error: notarize.sh takes an .app, a .dmg or a .zip, not $file" >&2
      exit 2
      ;;
  esac
done
