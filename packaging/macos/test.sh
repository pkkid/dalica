#!/usr/bin/env bash
# The macOS packaging checks that run on any OS (Linux too), in seconds, without building:
#
#   - dmg/generate.py --self-test: the Craft apps' committed .DS_Store files reproduced byte for
#     byte, and Dalica's layout identical across two runs;
#   - two runs into separate folders give identical files; a broken icon file is refused;
#   - Info.plist.in rendered for a release and a pre-release version: well-formed (xmllint when
#     installed), every placeholder filled, the bundle id, versions and executable as expected;
#   - common.sh refuses a malformed version and commit;
#   - bash -n on every script, and shellcheck when installed (CI's Packaging lint runs it always).
#
# Usage: packaging/macos/test.sh      (package.sh runs it first)
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/dalica-macos-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

echo "==> dmg/generate.py"
python3 "$HERE/dmg/generate.py" --self-test
python3 "$HERE/dmg/generate.py" "$TMP/a" >/dev/null
python3 "$HERE/dmg/generate.py" "$TMP/b" >/dev/null
for f in background.tiff DS_Store; do
  cmp "$TMP/a/$f" "$TMP/b/$f" || fail "two runs wrote different $f"
done
echo "ok: two runs into separate folders are identical"
# A wrong icon file is an error message and status 1, never a traceback or a partial layout.
printf 'not a png' >"$TMP/bad.png"
head -c 100 "$HERE/../../assets/app-icon/dalica-1024.png" >"$TMP/cut.png"
for icon in "$TMP/bad.png" "$TMP/cut.png" "$TMP/missing.png"; do
  status=0
  python3 "$HERE/dmg/generate.py" --icon "$icon" "$TMP/c" 2>"$TMP/err" || status=$?
  if [ "$status" != 1 ] || ! grep -q '^error: ' "$TMP/err" || [ -e "$TMP/c/background.tiff" ]; then
    fail "generate.py with $(basename "$icon"): status $status, $(cat "$TMP/err")"
  fi
done
echo "ok: a broken icon file is refused"

echo "==> Info.plist.in"
for v in 0.1.0 0.2.0-rc.1; do
  out="$TMP/Info-$v.plist"
  (
    export DALICA_VERSION="$v" DALICA_BUILD_SHA=0123456789abcdef0123456789abcdef01234567
    # shellcheck source=common.sh
    . "$HERE/common.sh"
    render_info_plist "$out"
    if command -v xmllint >/dev/null; then xmllint --noout "$out"; fi
    python3 - "$out" "$APP_ID" "$v" <<'EOF'
import plistlib, sys
path, app_id, version = sys.argv[1:]
with open(path, "rb") as f:
    p = plistlib.load(f)
short = version.split("-")[0]
want = {
    "CFBundleIdentifier": app_id,
    "CFBundleExecutable": "Dalica",
    "CFBundleName": "Dalica",
    "CFBundleIconFile": "Dalica",
    "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": short,
    "CFBundleVersion": short,
    "DalicaVersion": version,
    "DalicaBuildCommit": "0123456789abcdef0123456789abcdef01234567",
    "LSMinimumSystemVersion": "11.0",
}
for key, value in want.items():
    if p.get(key) != value:
        sys.exit(f"FAIL: {path}: {key} is {p.get(key)!r}, expected {value!r}")
types = p["CFBundleDocumentTypes"]
if not types or any(t.get("LSHandlerRank") != "Alternate" for t in types):
    sys.exit(f"FAIL: {path}: every document type must be an Alternate handler")
if "public.jpeg" not in types[0]["LSItemContentTypes"]:
    sys.exit(f"FAIL: {path}: JPEG is not among the document types")
print(f"ok: Info.plist for {version}: {app_id}, {short}")
EOF
  ) || fail "Info.plist for $v"
done
[ "$(grep -c '@[A-Z_]*@' "$HERE/Info.plist.in")" -ge 4 ] || fail "Info.plist.in lost its placeholders"

echo "==> common.sh refuses bad input"
for bad in "DALICA_VERSION=1.2" "DALICA_VERSION=1.2.3;rm" "DALICA_BUILD_SHA=not-a-sha"; do
  # shellcheck source=common.sh
  if (export "${bad?}"; . "$HERE/common.sh") 2>/dev/null; then
    fail "common.sh accepted $bad"
  fi
  echo "ok: refused $bad"
done

echo "==> scripts"
for s in "$HERE"/*.sh; do
  bash -n "$s" || fail "bash -n $s"
done
if command -v shellcheck >/dev/null; then
  shellcheck --severity=style -x -- "$HERE"/*.sh
  echo "ok: shellcheck"
else
  echo "skipped: shellcheck is not installed here (the Packaging lint workflow runs it)"
fi
if [ "$(uname -s)" != Darwin ]; then
  echo "skipped: building, signing, the DMG and verify.sh need macOS (package.sh in the release workflow)"
fi
echo "==> all macOS packaging checks passed"
