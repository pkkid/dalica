# shellcheck shell=bash
# shellcheck disable=SC2034 # the variables set here are package.sh's
# The packaging tools for package.sh, at pinned versions checked against their SHA-256. Sourced by
# package.sh after it sets ARCH (x86_64 or aarch64) and TOOLS (the folder downloads go to).
#
#   install_tools   (CI, or package.sh --install-tools) the distribution packages through apt, nfpm
#                   from its .deb, appimagetool and the AppImage runtime into $TOOLS
#   find_tools      sets NFPM, APPIMAGETOOL, APPIMAGE_RUNTIME and FLATPAK to what is usable here
#                   (empty when missing); never downloads
#
# Adapted from PhotoCraft packaging/linux/verified-download.sh and packaging/linux/package.sh
# @ 2bf49f10 (MIT OR Apache-2.0): the verified download and the appimagetool and runtime pins.
# Changed for Dalica: nfpm pinned by checksum too, and one place that installs every tool.

# appimagetool and the static type2 runtime, pinned together (appimagetool would otherwise fetch the
# newest runtime itself). Digests from the GitHub release assets:
# https://github.com/AppImage/appimagetool/releases/tag/1.9.1
# https://github.com/AppImage/type2-runtime/releases/tag/20251108
APPIMAGETOOL_VERSION=1.9.1
APPIMAGE_RUNTIME_VERSION=20251108
# nfpm (https://github.com/goreleaser/nfpm/releases/tag/v2.47.0), the .deb of the release.
NFPM_VERSION=2.47.0

pins_for_arch() {
  case "$ARCH" in
    x86_64)
      APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
      APPIMAGE_RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
      NFPM_DEB_ARCH=amd64
      NFPM_SHA256=3f1cf344bd0b57373ca55636a78c08b0491f7293d609a456a9ac3b0b150fda97
      ;;
    aarch64)
      APPIMAGETOOL_SHA256=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
      APPIMAGE_RUNTIME_SHA256=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444
      NFPM_DEB_ARCH=arm64
      NFPM_SHA256=27419eb382695a7942be8ad52259f3ec1854fad001b3ae4baed34ce39a223b97
      ;;
    *)
      echo "error: no tool pins for $ARCH" >&2
      return 1
      ;;
  esac
}

# The SHA-256 of a file, alone.
sha256() {
  sha256sum "$1" | cut -d' ' -f1
}

# Download $1 to $2 unless $2 already has the SHA-256 $3; refuse a download with another digest.
# $4=true makes the file executable.
download_verified() {
  local url="$1" dest="$2" expected="$3" executable="${4:-false}" tmp
  if [ -f "$dest" ] && [ "$(sha256 "$dest")" = "$expected" ]; then
    if [ "$executable" = true ]; then chmod 755 "$dest"; fi
    return 0
  fi
  mkdir -p "$(dirname "$dest")"
  tmp="$(mktemp "$dest.XXXXXX")"
  if ! curl -fsSL --retry 3 -o "$tmp" "$url"; then
    rm -f "$tmp"
    echo "error: could not download $url" >&2
    return 1
  fi
  if [ "$(sha256 "$tmp")" != "$expected" ]; then
    echo "error: SHA-256 mismatch for $url (expected $expected)" >&2
    rm -f "$tmp"
    return 1
  fi
  if [ "$executable" = true ]; then chmod 755 "$tmp"; else chmod 644 "$tmp"; fi
  mv -f "$tmp" "$dest"
}

# The verified files' paths in $TOOLS.
tool_paths() {
  APPIMAGETOOL_FILE="$TOOLS/appimagetool-$APPIMAGETOOL_VERSION-$ARCH.AppImage"
  APPIMAGE_RUNTIME_FILE="$TOOLS/type2-runtime-$APPIMAGE_RUNTIME_VERSION-$ARCH"
}

install_tools() {
  pins_for_arch
  tool_paths
  echo "==> installing the packaging tools (apt, nfpm $NFPM_VERSION, appimagetool $APPIMAGETOOL_VERSION)"
  sudo apt-get update
  # desktop-file-utils and appstream validate the metadata; zsync writes the AppImage's .zsync;
  # flatpak builds and tests the bundle; rpm lists the .rpm's files.
  sudo apt-get install -y --no-install-recommends desktop-file-utils appstream zsync flatpak rpm
  local deb="$TOOLS/nfpm_${NFPM_VERSION}_$NFPM_DEB_ARCH.deb"
  download_verified "https://github.com/goreleaser/nfpm/releases/download/v$NFPM_VERSION/nfpm_${NFPM_VERSION}_$NFPM_DEB_ARCH.deb" \
    "$deb" "$NFPM_SHA256"
  sudo dpkg -i "$deb"
  download_verified "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$ARCH.AppImage" \
    "$APPIMAGETOOL_FILE" "$APPIMAGETOOL_SHA256" true
  download_verified "https://github.com/AppImage/type2-runtime/releases/download/$APPIMAGE_RUNTIME_VERSION/runtime-$ARCH" \
    "$APPIMAGE_RUNTIME_FILE" "$APPIMAGE_RUNTIME_SHA256"
}

# Sets each tool's variable to its command or file when it is here and right, else to empty, and
# WHY_<TOOL> to the reason.
find_tools() {
  pins_for_arch
  tool_paths
  NFPM="" APPIMAGETOOL="" APPIMAGE_RUNTIME="" FLATPAK=""
  if command -v nfpm >/dev/null; then NFPM=nfpm; else WHY_NFPM="nfpm not found"; fi
  if [ -f "$APPIMAGETOOL_FILE" ] && [ "$(sha256 "$APPIMAGETOOL_FILE")" = "$APPIMAGETOOL_SHA256" ] &&
    [ -f "$APPIMAGE_RUNTIME_FILE" ] && [ "$(sha256 "$APPIMAGE_RUNTIME_FILE")" = "$APPIMAGE_RUNTIME_SHA256" ]; then
    chmod 755 "$APPIMAGETOOL_FILE"
    APPIMAGETOOL="$APPIMAGETOOL_FILE"
    APPIMAGE_RUNTIME="$APPIMAGE_RUNTIME_FILE"
  else
    WHY_APPIMAGE="appimagetool $APPIMAGETOOL_VERSION and runtime $APPIMAGE_RUNTIME_VERSION are not in $TOOLS"
  fi
  if command -v flatpak >/dev/null; then FLATPAK=flatpak; else WHY_FLATPAK="flatpak not found"; fi
}
