#!/usr/bin/env bash
# Build and package Dalica for macOS 11 and later, universal (Apple silicon + Intel):
#
#   $DIST/dalica-<version>-macos-universal.zip       dalica (stands in for the DMG until M6.23)
#   $DIST/dalica-cli-<version>-macos-universal.zip   dalica-cli
#
# Usage: packaging/macos/package.sh [--skip-build]
#
# A stub (M6.13): it builds both architectures, joins them with lipo and zips the universal
# binaries with the licence files, so the release workflow runs whole; M6.23 assembles Dalica.app
# and the DMG (docs/releasing.md → macOS), and M15 signs and notarizes. Adapted from LightCraft
# packaging/env.sh and packaging/macos/package.sh @ 25ef65c8 (MIT OR Apache-2.0): the version,
# provenance and licence helpers and the two-target lipo build, cut down to the zips.
#
# Needs: Xcode command line tools (lipo, ditto) and the Rust targets aarch64-apple-darwin and
# x86_64-apple-darwin. Environment (all optional): DALICA_VERSION, DALICA_BUILD_SHA,
# DALICA_BUILD_DATE, DIST, CARGO_TARGET_DIR, CRAFT_FONTS_DIR (as packaging/linux/package.sh).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

SKIP_BUILD=0
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]"); next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
}
VERSION="${DALICA_VERSION:-$(workspace_version)}"
[ -n "$VERSION" ] || { echo "error: no [workspace.package] version in $ROOT/Cargo.toml" >&2; exit 1; }

# Provenance baked into the binaries (crates/core/src/build.rs).
DALICA_BUILD_SHA="${DALICA_BUILD_SHA:-$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || true)}"
DALICA_BUILD_DATE="${DALICA_BUILD_DATE:-$(date -u +%Y-%m-%d)}"
export DALICA_BUILD_SHA DALICA_BUILD_DATE
export MACOSX_DEPLOYMENT_TARGET=11.0

DIST="${DIST:-$ROOT/dist/release}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
mkdir -p "$DIST"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
echo "==> Dalica $VERSION for macOS universal (${DALICA_BUILD_SHA:0:9}, $DALICA_BUILD_DATE)"

if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p dalica -p dalica-cli --target "${TARGETS[0]}" --target "${TARGETS[1]}")
fi

WORK="$TARGET_DIR/macos-package"
rm -rf "$WORK"
mkdir -p "$WORK/universal"
for exe in dalica dalica-cli; do
  lipo -create -output "$WORK/universal/$exe" \
    "$TARGET_DIR/${TARGETS[0]}/release/$exe" "$TARGET_DIR/${TARGETS[1]}/release/$exe"
  lipo "$WORK/universal/$exe" -verify_arch arm64 x86_64
  lipo -info "$WORK/universal/$exe"
  # It must report this version and the commit, not a dev build.
  out="$("$WORK/universal/$exe" --version)"
  echo "$exe --version: $out"
  case "$out" in *"$VERSION ("*) ;; *) echo "error: $exe reports '$out', not $VERSION" >&2; exit 1 ;; esac
  if [ -n "$DALICA_BUILD_SHA" ]; then
    case "$out" in *"${DALICA_BUILD_SHA:0:9}"*) ;; *) echo "error: $exe does not report commit ${DALICA_BUILD_SHA:0:9}" >&2; exit 1 ;; esac
  fi
done

# One zip per binary: <name>/<binary> plus the licences, the embedded Inter's OFL and the OFL of
# every craft-fonts family a CRAFT_FONTS_DIR build embeds.
pack() { # pack EXE ZIPBASE
  local dir="$WORK/$2" f lic
  mkdir -p "$dir"
  cp "$WORK/universal/$1" "$dir/"
  for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md README.md; do
    cp "$ROOT/$f" "$dir/"
  done
  cp "$ROOT/assets/fonts/OFL-Inter.txt" "$dir/"
  if [ -n "${CRAFT_FONTS_DIR:-}" ]; then
    for lic in "$CRAFT_FONTS_DIR"/fonts/*/OFL.txt; do
      [ -f "$lic" ] || continue
      cp "$lic" "$dir/OFL-$(basename "$(dirname "$lic")").txt"
    done
  fi
  rm -f "$DIST/$2.zip"
  # ditto keeps the executable bit and makes the zip Finder's Archive Utility expects.
  ditto -c -k --keepParent "$dir" "$DIST/$2.zip"
  echo "wrote $DIST/$2.zip"
  unzip -l "$DIST/$2.zip"
}
pack dalica "dalica-$VERSION-macos-universal"
pack dalica-cli "dalica-cli-$VERSION-macos-universal"
