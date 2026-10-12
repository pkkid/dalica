#!/usr/bin/env bash
# Build and package Dalica for Linux (<arch>: x86_64 or aarch64, the machine's own):
#
#   $DIST/dalica-<version>-linux-<arch>.tar.gz   dalica and dalica-cli, with the licence files
#
# Usage: packaging/linux/package.sh [--skip-build]
#
# A stub (M6.13): it packs the release binaries into the tarball so the release workflow runs
# whole; M6.21 stages the FHS tree and adds the AppImage, .deb, .rpm and Flatpak bundle
# (docs/releasing.md → Linux). Adapted from LightCraft packaging/env.sh and
# packaging/linux/package.sh @ 25ef65c8 (MIT OR Apache-2.0): the version, provenance and licence
# helpers, cut down to the tarball.
#
# Environment (all optional): DALICA_VERSION (default: [workspace.package] version of Cargo.toml),
# DALICA_BUILD_SHA (default: git HEAD), DALICA_BUILD_DATE (default: today, UTC), DIST (default:
# dist/release), CARGO_TARGET_DIR, CRAFT_FONTS_DIR (its fonts' OFL.txt files ship as
# OFL-<family>.txt).
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

# The version lives in one place: [workspace.package] version in the root Cargo.toml (awk, so
# packaging never compiles xtask to read it).
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

DIST="${DIST:-$ROOT/dist/release}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
mkdir -p "$DIST"

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) echo "error: unsupported architecture $ARCH" >&2; exit 2 ;;
esac
BASENAME="dalica-$VERSION-linux-$ARCH"
echo "==> Dalica $VERSION for Linux $ARCH (${DALICA_BUILD_SHA:0:9}, $DALICA_BUILD_DATE)"

if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p dalica -p dalica-cli)
fi
BIN="$TARGET_DIR/release"

# Both binaries must report this version and the commit, not a dev build.
for exe in dalica dalica-cli; do
  out="$("$BIN/$exe" --version)"
  echo "$exe --version: $out"
  case "$out" in *"$VERSION ("*) ;; *) echo "error: $exe reports '$out', not $VERSION" >&2; exit 1 ;; esac
  if [ -n "$DALICA_BUILD_SHA" ]; then
    case "$out" in *"${DALICA_BUILD_SHA:0:9}"*) ;; *) echo "error: $exe does not report commit ${DALICA_BUILD_SHA:0:9}" >&2; exit 1 ;; esac
  fi
done

WORK="$TARGET_DIR/linux-package"
STAGE="$WORK/$BASENAME"
rm -rf "$WORK"
mkdir -p "$STAGE/bin" "$STAGE/share/doc/dalica"
install -m755 "$BIN/dalica" "$BIN/dalica-cli" "$STAGE/bin/"
strip "$STAGE/bin/dalica" "$STAGE/bin/dalica-cli" 2>/dev/null || true

# Every package ships the licences (docs/releasing.md → What gets built), the embedded Inter's OFL,
# and the OFL of every craft-fonts family a CRAFT_FONTS_DIR build embeds.
DOC="$STAGE/share/doc/dalica"
for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md README.md; do
  cp "$ROOT/$f" "$DOC/"
done
cp "$ROOT/assets/fonts/OFL-Inter.txt" "$DOC/"
if [ -n "${CRAFT_FONTS_DIR:-}" ]; then
  for lic in "$CRAFT_FONTS_DIR"/fonts/*/OFL.txt; do
    [ -f "$lic" ] || continue
    cp "$lic" "$DOC/OFL-$(basename "$(dirname "$lic")").txt"
  done
fi

tar -C "$WORK" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
echo "wrote $DIST/$BASENAME.tar.gz"
tar -tzf "$DIST/$BASENAME.tar.gz"
