# shellcheck shell=bash disable=SC2034  # the variables are for the scripts that source this file
# Shared setup for the macOS packaging scripts. Source it: `. "$(dirname "${BASH_SOURCE[0]}")/common.sh"`.
#
# Sets: ROOT, HERE (packaging/macos), VERSION, SHORT_VERSION (numeric X.Y.Z for CFBundleVersion),
# APP_ID (crates/core/src/app_id.rs), DIST, TARGET_DIR, WORK, DMG, CLI_ZIP, CLI_DIR_NAME, and
# exports DALICA_BUILD_SHA, DALICA_BUILD_DATE and MACOSX_DEPLOYMENT_TARGET. Defines warn,
# render_info_plist and copy_licences. Needs nothing beyond bash, awk and sed, so the template
# rendering also runs on Linux (test.sh).
#
# Adapted from PhotoCraft packaging/env.sh @ 77221725 and LightCraft packaging/env.sh @ 25ef65c8
# (MIT OR Apache-2.0): the version, provenance, warning and licence helpers.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HERE="$ROOT/packaging/macos"

# Emit a GitHub Actions warning (plain stderr outside Actions).
warn() {
  if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]"); next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
}
VERSION="${DALICA_VERSION:-$(workspace_version)}"
# Checked before it reaches sed, file names or the plist: X.Y.Z with an optional -pre part.
re='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'
if ! [[ $VERSION =~ $re ]]; then
  echo "error: '$VERSION' is not a version like 1.2.3 or 1.2.3-rc.1" >&2
  exit 1
fi
SHORT_VERSION="${VERSION%%-*}"

# The application id, from its one definition (crates/core/src/app_id.rs, checked there to be a
# valid bundle id; checked again here because it reaches sed and codesign).
APP_ID="$(sed -n 's/^pub const APP_ID: &str = "\([^"]*\)";$/\1/p' "$ROOT/crates/core/src/app_id.rs")"
re='^[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+$'
if ! [[ $APP_ID =~ $re ]]; then
  echo "error: no valid APP_ID in crates/core/src/app_id.rs (got '$APP_ID')" >&2
  exit 1
fi

# Provenance baked into the binaries (crates/core/src/build.rs).
DALICA_BUILD_SHA="${DALICA_BUILD_SHA:-$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || true)}"
DALICA_BUILD_DATE="${DALICA_BUILD_DATE:-$(date -u +%Y-%m-%d)}"
re='^[0-9a-f]*$'
if ! [[ $DALICA_BUILD_SHA =~ $re ]] || [ "${#DALICA_BUILD_SHA}" -gt 64 ]; then
  echo "error: DALICA_BUILD_SHA '$DALICA_BUILD_SHA' is not a hexadecimal commit" >&2
  exit 1
fi
export DALICA_BUILD_SHA DALICA_BUILD_DATE
# Keep in sync with LSMinimumSystemVersion in Info.plist.in.
export MACOSX_DEPLOYMENT_TARGET=11.0

DIST="${DIST:-$ROOT/dist/release}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
WORK="$TARGET_DIR/macos-package"
DMG="$DIST/dalica-$VERSION-macos-universal.dmg"
CLI_DIR_NAME="dalica-cli-$VERSION-macos-universal"
CLI_ZIP="$DIST/$CLI_DIR_NAME.zip"

# render_info_plist OUT: Info.plist.in with the version, commit and application id filled in.
render_info_plist() {
  sed -e "s/@APP_ID@/$APP_ID/g" -e "s/@VERSION@/$VERSION/g" \
    -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" -e "s/@BUILD_SHA@/${DALICA_BUILD_SHA:-unknown}/g" \
    "$HERE/Info.plist.in" >"$1"
  if grep -q '@[A-Z_]*@' "$1"; then
    echo "error: $1 still has a placeholder: $(grep -o '@[A-Z_]*@' "$1" | head -n 1)" >&2
    return 1
  fi
}

# copy_licences DEST: the licences, README, the embedded Inter's OFL and the OFL of every
# craft-fonts family a CRAFT_FONTS_DIR build embeds (as OFL-<family>.txt).
copy_licences() {
  local dest="$1" f lic
  mkdir -p "$dest"
  for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md README.md; do
    cp "$ROOT/$f" "$dest/"
  done
  cp "$ROOT/assets/fonts/OFL-Inter.txt" "$dest/"
  if [ -n "${CRAFT_FONTS_DIR:-}" ]; then
    for lic in "$CRAFT_FONTS_DIR"/fonts/*/OFL.txt; do
      [ -f "$lic" ] || continue
      cp "$lic" "$dest/OFL-$(basename "$(dirname "$lic")").txt"
    done
  fi
}
