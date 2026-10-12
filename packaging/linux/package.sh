#!/usr/bin/env bash
# Build and package Dalica for Linux (<arch>: x86_64 or aarch64, the machine's own), every format
# from one staged FHS tree (docs/releasing.md → Linux):
#
#   $DIST/dalica-<version>-linux-<arch>.tar.gz          bin/ and share/, for /opt or ~/.local
#   $DIST/dalica-<version>-linux-<arch>.deb             Debian, Ubuntu, Mint, ...
#   $DIST/dalica-<version>-linux-<arch>.rpm             Fedora, openSUSE, RHEL, ...
#   $DIST/dalica-<version>-linux-<arch>.AppImage        any distribution with glibc >= the build's
#   $DIST/dalica-<version>-linux-<arch>.AppImage.zsync  the AppImage's delta updates
#   $DIST/dalica-<version>-linux-<arch>.flatpak         the Flatpak bundle, made from the tarball
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "tar deb rpm appimage flatpak"]
#                                   [--install-tools] [--require-all]
#
# Tools: nfpm (.deb, .rpm), appimagetool and the AppImage runtime, zsyncmake (the .zsync),
# flatpak, desktop-file-validate and appstreamcli. --install-tools installs them at pinned,
# checksum-verified versions (tools.sh; apt and sudo). Without it nothing is downloaded: a format
# whose tool is missing is skipped and the summary says so. --require-all makes a skipped format
# or check an error. In GitHub Actions both are on (DALICA_INSTALL_TOOLS=0 turns them off).
#
# Checks: both binaries report the version and the commit; `dalica` refuses to start on a planted
# system missing a library, naming the package; the desktop entry and the AppStream metainfo
# validate; AppRun's desktop integration (apprun-test.sh); every format holds the staged tree's
# files exactly; the AppImage runs `dalica --version` and carries its update information; the
# Flatpak's permissions are the manifest's, and it runs when its runtime is installed. With
# --install-tools also: the .deb installs, runs and uninstalls cleanly, and the Flatpak bundle
# installs into a private installation (with the runtime from Flathub) and runs.
#
# Environment (all optional): DALICA_VERSION (default: [workspace.package] version of Cargo.toml),
# DALICA_BUILD_SHA (default: git HEAD), DALICA_BUILD_DATE (default: today, UTC), DIST (default:
# dist/release), CARGO_TARGET_DIR, CRAFT_FONTS_DIR (its fonts' OFL.txt files ship as
# OFL-<family>.txt), DALICA_MAINTAINER (the packages' maintainer field), GITHUB_REPOSITORY (the
# AppImage's update source; default pkkid/dalica).
#
# Adapted from LightCraft packaging/env.sh and packaging/linux/package.sh @ 25ef65c8 and PhotoCraft
# packaging/linux/package.sh @ 2bf49f10 and packaging/linux/flatpak-bundle.sh @ 231bf5fe (MIT OR
# Apache-2.0): the staged tree, the nfpm, appimagetool and update-information steps, and the
# Flatpak made from the tarball. Changed for Dalica: the Flatpak is built with plain
# `flatpak build-*` commands (no SDK), the script installs its own tools, and the checks above.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HERE="$ROOT/packaging/linux"
APP_ID=org.dalica.Dalica
MANIFEST="$HERE/flatpak/$APP_ID.bundle.yml"
FLATHUB_REPO=https://dl.flathub.org/repo/flathub.flatpakrepo
# shellcheck source=tools.sh
. "$HERE/tools.sh"

ci_default=0
if [ "${GITHUB_ACTIONS:-}" = true ]; then ci_default=1; fi
INSTALL_TOOLS="${DALICA_INSTALL_TOOLS:-$ci_default}"
REQUIRE_ALL="${DALICA_INSTALL_TOOLS:-$ci_default}"
SKIP_BUILD=0
FORMATS="tar deb rpm appimage flatpak"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="${2:-}"; shift 2 ;;
    --install-tools) INSTALL_TOOLS=1; shift ;;
    --require-all) REQUIRE_ALL=1; shift ;;
    -h | --help) sed -n '2,37p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
for f in $FORMATS; do
  case "$f" in
    tar | deb | rpm | appimage | flatpak) ;;
    *) echo "unknown format: $f (tar deb rpm appimage flatpak)" >&2; exit 2 ;;
  esac
done
has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

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
mkdir -p "$DIST" "$TARGET_DIR"
DIST="$(cd "$DIST" && pwd)"
TARGET_DIR="$(cd "$TARGET_DIR" && pwd)"
TOOLS="$TARGET_DIR/packaging-tools"

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "error: unsupported architecture $ARCH" >&2; exit 2 ;;
esac
BASENAME="dalica-$VERSION-linux-$ARCH"
echo "==> Dalica $VERSION for Linux $ARCH (${DALICA_BUILD_SHA:0:9}, $DALICA_BUILD_DATE): $FORMATS"

# What was made, and what was skipped and why, for the summary.
BUILT=()
SKIPPED=()
skip() {
  SKIPPED+=("$1: $2")
  if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning::$1 skipped: $2"; else echo "warning: $1 skipped: $2" >&2; fi
}
fail() {
  echo "error: $*" >&2
  exit 1
}

if [ "$INSTALL_TOOLS" = 1 ]; then install_tools; fi
find_tools

if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p dalica -p dalica-cli)
fi
BIN="$TARGET_DIR/release"

# `check_version LABEL COMMAND...`: COMMAND --version reports this version and the commit, not a
# dev build.
check_version() {
  local label="$1" out
  shift
  out="$("$@" --version)"
  echo "$label --version: $out"
  case "$out" in *"$VERSION ("*) ;; *) fail "$label reports '$out', not $VERSION" ;; esac
  if [ -n "$DALICA_BUILD_SHA" ]; then
    case "$out" in *"${DALICA_BUILD_SHA:0:9}"*) ;; *) fail "$label does not report commit ${DALICA_BUILD_SHA:0:9}" ;; esac
  fi
}
for exe in dalica dalica-cli; do check_version "$exe" "$BIN/$exe"; done

WORK="$TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"
mkdir -p "$WORK"

# ---- the start-up check, on planted systems without a library -----------------------------------
# `dalica` reads the loader's configuration under DALICA_LIB_CHECK_ROOT
# (apps/dalica/src/preflight.rs); each planted system lacks a library its session loads.
plant_system() {
  local root="$1" os_release="$2" lib
  shift 2
  mkdir -p "$root/etc/ld.so.conf.d" "$root/opt/gfx/lib"
  printf '%s\n' "$os_release" >"$root/etc/os-release"
  printf 'include ld.so.conf.d/*.conf\n' >"$root/etc/ld.so.conf"
  printf '/opt/gfx/lib\n' >"$root/etc/ld.so.conf.d/gfx.conf"
  for lib in "$@"; do : >"$root/opt/gfx/lib/$lib"; done
}
# `expect_refusal WHAT ROOT TEXT VAR=VALUE...`: dalica exits 1 and prints TEXT.
expect_refusal() {
  local what="$1" root="$2" want="$3" out code
  shift 3
  set +e
  out="$(env -i PATH="$PATH" DALICA_LIB_CHECK_ROOT="$root" "$@" timeout 60 "$BIN/dalica" 2>&1)"
  code=$?
  set -e
  [ "$code" = 1 ] || fail "start-up check ($what): exit status $code, not 1: $out"
  case "$out" in *"$want"*) ;; *) fail "start-up check ($what) does not say '$want': $out" ;; esac
  echo "start-up check ($what): exit status 1, names '$want'"
}
plant_system "$WORK/preflight-x11" "ID=ubuntu
ID_LIKE=debian" libX11.so.6 libX11-xcb.so.1 libxcb.so.1 libXcursor.so.1 libXi.so.6 libxkbcommon.so.0 libvulkan.so.1
expect_refusal "X11 without xkbcommon-x11" "$WORK/preflight-x11" "sudo apt install libxkbcommon-x11-0" DISPLAY=:99
plant_system "$WORK/preflight-wayland" "ID=fedora"
expect_refusal "Wayland without its libraries" "$WORK/preflight-wayland" \
  "sudo dnf install libwayland-client libxkbcommon vulkan-loader" WAYLAND_DISPLAY=wayland-99

# ---- stage the FHS tree (every format is made from it) ------------------------------------------
install -Dm755 "$BIN/dalica" "$STAGE/usr/bin/dalica"
install -Dm755 "$BIN/dalica-cli" "$STAGE/usr/bin/dalica-cli"
strip "$STAGE/usr/bin/dalica" "$STAGE/usr/bin/dalica-cli" 2>/dev/null || true
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
mkdir -p "$STAGE/usr/share/metainfo" "$STAGE/usr/share/icons" "$STAGE/usr/share/doc/dalica"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$DALICA_BUILD_DATE/g" \
  "$HERE/$APP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
# hicolor 16-512 px and the scalable SVG, named by the application id (packaging/icons.sh).
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/usr/share/icons/"
# Every package ships the licences (docs/releasing.md → What gets built), the embedded Inter's OFL,
# and the OFL of every craft-fonts family a CRAFT_FONTS_DIR build embeds.
DOC="$STAGE/usr/share/doc/dalica"
for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md README.md; do
  install -m644 "$ROOT/$f" "$DOC/"
done
install -m644 "$ROOT/assets/fonts/OFL-Inter.txt" "$DOC/"
if [ -n "${CRAFT_FONTS_DIR:-}" ]; then
  for lic in "$CRAFT_FONTS_DIR"/fonts/*/OFL.txt; do
    [ -f "$lic" ] || continue
    install -m644 "$lic" "$DOC/OFL-$(basename "$(dirname "$lic")").txt"
  done
fi

# The tree's files, relative to usr/ and sorted: what every format must hold, exactly.
list_files() {
  (cd "$1" && find . \( -type f -o -type l \) | sed 's|^\./||' | LC_ALL=C sort)
}
EXPECTED="$WORK/expected-files.txt"
list_files "$STAGE/usr" >"$EXPECTED"
same_files() {
  local what="$1" got="$2"
  if ! diff -u "$EXPECTED" "$got" >"$WORK/layout.diff"; then
    cat "$WORK/layout.diff" >&2
    fail "$what does not hold the staged tree's files (above: - missing, + extra)"
  fi
  echo "$what: holds the staged tree's $(wc -l <"$EXPECTED") files"
}

# ---- metadata -----------------------------------------------------------------------------------
if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
  echo "desktop-file-validate: ok"
else
  skip "desktop-file-validate" "not installed (desktop-file-utils)"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
else
  skip "appstreamcli validate" "not installed (appstream)"
fi

# ---- .tar.gz (always made: the Flatpak is made from it) -----------------------------------------
TARBALL="$WORK/$BASENAME.tar.gz"
mkdir -p "$WORK/tar"
cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
tar -C "$WORK/tar" --owner=0 --group=0 --numeric-owner -czf "$TARBALL" "$BASENAME"
tar -tzf "$TARBALL" | grep -v '/$' | sed "s|^$BASENAME/||" | LC_ALL=C sort >"$WORK/tar-files.txt"
same_files ".tar.gz" "$WORK/tar-files.txt"
if has tar; then
  cp "$TARBALL" "$DIST/"
  BUILT+=("$DIST/$BASENAME.tar.gz")
fi

# ---- .deb and .rpm ------------------------------------------------------------------------------
if has deb || has rpm; then
  if [ -z "$NFPM" ]; then
    if has deb; then skip ".deb" "$WHY_NFPM"; fi
    if has rpm; then skip ".rpm" "$WHY_NFPM"; fi
  else
    # The glibc the binaries need (their newest GLIBC_ symbol version) is the .deb's floor.
    glibc="$(objdump -T "$STAGE/usr/bin/dalica" "$STAGE/usr/bin/dalica-cli" 2>/dev/null |
      grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n 1 || true)"
    glibc="${glibc:-2.35}"
    echo "glibc floor: $glibc"
    export VERSION NFPM_ARCH="$DEB_ARCH"
    export DALICA_MAINTAINER="${DALICA_MAINTAINER:-The Dalica contributors <https://github.com/pkkid/dalica>}"
    # nfpm expands variables in fields like `version` and `arch`, not in `contents[].src`.
    sed -e "s|\${STAGE}|$STAGE|g" -e "s|libc6 (>= 2.35)|libc6 (>= $glibc)|" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
    for fmt in deb rpm; do
      has "$fmt" || continue
      (cd "$ROOT" && "$NFPM" package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt")
      BUILT+=("$DIST/$BASENAME.$fmt")
    done
    if has deb; then
      if command -v dpkg-deb >/dev/null; then
        dpkg-deb --field "$DIST/$BASENAME.deb" Package Version Architecture Depends
        dpkg-deb -c "$DIST/$BASENAME.deb" | awk '$1 ~ /^-/ { print $6 }' | sed 's|^\./usr/||' | LC_ALL=C sort >"$WORK/deb-files.txt"
        same_files ".deb" "$WORK/deb-files.txt"
      else
        skip ".deb file check" "dpkg-deb not installed"
      fi
    fi
    if has rpm; then
      if command -v rpm >/dev/null; then
        rpm -qp --qf '%{NAME} %{VERSION}-%{RELEASE} %{ARCH}\n' "$DIST/$BASENAME.rpm"
        rpm -qp --qf '[%{FILEMODES:perms} %{FILENAMES}\n]' "$DIST/$BASENAME.rpm" |
          awk '$1 ~ /^-/ { print $2 }' | sed 's|^/usr/||' | LC_ALL=C sort >"$WORK/rpm-files.txt"
        same_files ".rpm" "$WORK/rpm-files.txt"
      else
        skip ".rpm file check" "rpm not installed"
      fi
    fi
  fi
fi

# ---- AppImage -----------------------------------------------------------------------------------
if has appimage; then
  # AppRun's desktop integration first: it runs on every launch of the AppImage.
  bash "$HERE/apprun-test.sh"
  if [ -z "$APPIMAGETOOL" ]; then
    skip "AppImage" "$WHY_APPIMAGE (package.sh --install-tools downloads them)"
  else
    APPDIR="$WORK/Dalica.AppDir"
    cp -R "$STAGE" "$APPDIR"
    install -m755 "$HERE/AppRun.sh" "$APPDIR/AppRun"
    cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
    cp "$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"
    ln -s "$APP_ID.png" "$APPDIR/.DirIcon"
    OUT="$DIST/$BASENAME.AppImage"
    # Update information: AppImageUpdate and the like read it from the file and fetch only the
    # blocks that changed in the newest published (non-pre-release) release, through the .zsync
    # beside each AppImage. A fork's builds point at its own releases.
    REPO="${GITHUB_REPOSITORY:-pkkid/dalica}"
    UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest|dalica-*-linux-$ARCH.AppImage.zsync"
    # A .zsync from an earlier run would hide a missing zsyncmake.
    rm -f "$OUT" "$OUT.zsync"
    # Extract-and-run: no FUSE needed here. The output embeds the static runtime, so users need no
    # libfuse2 either. appimagetool writes the .zsync into its working folder, hence the cd.
    (cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" --no-appstream \
      --runtime-file "$APPIMAGE_RUNTIME" -u "$UPDATE_INFO" "$APPDIR" "$OUT")
    BUILT+=("$OUT")
    if [ -s "$OUT.zsync" ]; then
      BUILT+=("$OUT.zsync")
    else
      skip "AppImage .zsync" "zsyncmake not found (the zsync package)"
    fi
    # The AppImage starts (without integrating into this machine's desktop) and says its version.
    check_version "dalica (AppImage)" env APPIMAGE_EXTRACT_AND_RUN=1 DALICA_NO_DESKTOP_INTEGRATION=1 "$OUT"
    info="$("$OUT" --appimage-updateinformation)"
    [ "$info" = "$UPDATE_INFO" ] || fail "the AppImage's update information is '$info', not '$UPDATE_INFO'"
    echo "AppImage update information: $info"
    mkdir -p "$WORK/appimage-extract"
    (cd "$WORK/appimage-extract" && "$OUT" --appimage-extract >/dev/null)
    list_files "$WORK/appimage-extract/squashfs-root/usr" >"$WORK/appimage-files.txt"
    same_files "AppImage" "$WORK/appimage-files.txt"
  fi
fi

# ---- Flatpak bundle, from the tarball -----------------------------------------------------------
# The manifest (flatpak/org.dalica.Dalica.bundle.yml) says what goes in. Its simple YAML is read
# here line by line: a top-level `key: value`, or the `- item` lines under a key.
manifest_value() {
  local v
  v="$(awk -v key="$1" 'index($0, key ":") == 1 { sub(/^[^:]*:[ \t]*/, ""); print; exit }' "$MANIFEST")"
  v="${v%\'}"
  v="${v#\'}"
  printf '%s' "$v"
}
manifest_list() {
  awk -v key="$1" '
    !inlist && $0 ~ ("^[ \t]*" key ":[ \t]*$") { inlist = 1; match($0, /^[ \t]*/); ind = RLENGTH; next }
    inlist {
      if ($0 ~ /^[ \t]*#/ || $0 ~ /^[ \t]*$/) next
      match($0, /^[ \t]*/)
      if ($0 ~ /^[ \t]*- / && RLENGTH >= ind) { item = $0; sub(/^[ \t]*- /, "", item); print item; next }
      exit
    }
  ' "$MANIFEST"
}
if has flatpak; then
  if [ -z "$FLATPAK" ]; then
    skip "Flatpak" "$WHY_FLATPAK"
  else
    FP="$WORK/flatpak"
    fp_id="$(manifest_value id)"
    fp_runtime="$(manifest_value runtime)"
    fp_runtime_version="$(manifest_value runtime-version)"
    fp_sdk="$(manifest_value sdk)"
    fp_command="$(manifest_value command)"
    mapfile -t fp_finish < <(manifest_list finish-args)
    mapfile -t fp_build < <(manifest_list build-commands)
    [ "$fp_id" = "$APP_ID" ] || fail "the Flatpak manifest's id is '$fp_id', not $APP_ID"
    if [ -z "$fp_runtime" ] || [ -z "$fp_runtime_version" ] || [ -z "$fp_sdk" ] || [ -z "$fp_command" ]; then
      fail "the Flatpak manifest lacks runtime, runtime-version, sdk or command"
    fi
    if [ "${#fp_finish[@]}" = 0 ] || [ "${#fp_build[@]}" = 0 ]; then
      fail "the Flatpak manifest lacks finish-args or build-commands"
    fi
    echo "Flatpak: $fp_id on $fp_runtime//$fp_runtime_version: ${fp_finish[*]}"
    mkdir -p "$FP/stage"
    tar -xzf "$TARBALL" -C "$FP/stage" --strip-components=1
    # What `flatpak build-init` makes, written here: older flatpaks (Ubuntu 22.04's 1.12) refuse
    # build-init without the SDK installed, and nothing here needs it.
    mkdir -p "$FP/build/files" "$FP/build/var/tmp" "$FP/build/var/run"
    printf '[Application]\nname=%s\nruntime=%s/%s/%s\nsdk=%s/%s/%s\n' "$fp_id" \
      "$fp_runtime" "$ARCH" "$fp_runtime_version" "$fp_sdk" "$ARCH" "$fp_runtime_version" >"$FP/build/metadata"
    for cmd in "${fp_build[@]}"; do
      (cd "$FP/stage" && FLATPAK_DEST="$FP/build/files" sh -ec "$cmd") || fail "Flatpak build command failed: $cmd"
    done
    list_files "$FP/build/files" >"$WORK/flatpak-files.txt"
    same_files "Flatpak" "$WORK/flatpak-files.txt"
    "$FLATPAK" build-finish --command="$fp_command" "${fp_finish[@]}" "$FP/build"
    # The sandbox is exactly this: Wayland with the X11 fallback, X11 shared memory, the GPU,
    # Pictures, and nothing else (no network, no home folder).
    metadata="$FP/build/metadata"
    context() { sed -n "s/^$1=//p" "$metadata" | tr ';' '\n' | sed '/^$/d' | LC_ALL=C sort | paste -sd' ' -; }
    for want in "command=$fp_command" "sockets=fallback-x11 wayland" "shared=ipc" "devices=dri" "filesystems=xdg-pictures"; do
      key="${want%%=*}"
      got="$(context "$key")"
      # Older flatpaks (1.12) also write `x11` beside `fallback-x11`, for flatpaks that do not
      # know the fallback; the fallback still decides at run time.
      if [ "$key" = sockets ] && [ "$got" = "fallback-x11 wayland x11" ]; then got="fallback-x11 wayland"; fi
      if [ "$got" != "${want#*=}" ]; then
        cat "$metadata" >&2
        fail "the Flatpak's $key is '$got', not '${want#*=}'"
      fi
    done
    echo "Flatpak sandbox: $(sed -n '/^\[Context\]/,$p' "$metadata" | tail -n +2 | paste -sd' ' -)"
    "$FLATPAK" build-export --arch="$ARCH" "$FP/repo" "$FP/build" stable
    OUT="$DIST/$BASENAME.flatpak"
    # --runtime-repo lets `flatpak install` fetch the freedesktop runtime from Flathub.
    "$FLATPAK" build-bundle --arch="$ARCH" --runtime-repo="$FLATHUB_REPO" "$FP/repo" "$OUT" "$fp_id" stable
    BUILT+=("$OUT")

    # Install it into a private installation (never the user's own) and run it: with the runtime
    # already installed on this system, or downloaded from Flathub with --install-tools.
    export FLATPAK_USER_DIR="$FP/user"
    if [ "$INSTALL_TOOLS" = 1 ]; then
      "$FLATPAK" remote-add --user --if-not-exists flathub "$FLATHUB_REPO"
      "$FLATPAK" install --user -y --noninteractive --no-related flathub "$fp_runtime//$fp_runtime_version"
    fi
    if "$FLATPAK" info "$fp_runtime//$fp_runtime_version" >/dev/null 2>&1; then
      "$FLATPAK" install --user -y --noninteractive --no-related --no-deps --bundle "$OUT"
      check_version "dalica (Flatpak)" "$FLATPAK" run --user --command=dalica "$fp_id"
      check_version "dalica-cli (Flatpak)" "$FLATPAK" run --user --command=dalica-cli "$fp_id"
      "$FLATPAK" uninstall --user -y --noninteractive "$fp_id"
    else
      skip "Flatpak run test" "the runtime $fp_runtime//$fp_runtime_version is not installed (--install-tools installs it)"
    fi
    unset FLATPAK_USER_DIR
  fi
fi

# ---- the .deb, installed and removed (--install-tools: CI) --------------------------------------
if has deb && [ -f "$DIST/$BASENAME.deb" ]; then
  if [ "$INSTALL_TOOLS" = 1 ]; then
    sudo apt-get install -y --no-install-recommends "$DIST/$BASENAME.deb"
    check_version "/usr/bin/dalica" /usr/bin/dalica
    check_version "/usr/bin/dalica-cli" /usr/bin/dalica-cli
    sudo apt-get remove -y dalica
    for f in /usr/bin/dalica /usr/bin/dalica-cli "/usr/share/applications/$APP_ID.desktop" /usr/share/doc/dalica; do
      [ ! -e "$f" ] || fail "$f is still there after removing the .deb"
    done
    echo ".deb: installs, runs and uninstalls cleanly"
  else
    skip ".deb install test" "needs --install-tools (it installs into this system with sudo)"
  fi
fi

# ---- summary ------------------------------------------------------------------------------------
echo "==> Dalica $VERSION for Linux $ARCH"
for f in "${BUILT[@]}"; do
  printf '  %-7s %s\n' "$(numfmt --to=iec "$(stat -c %s "$f")")" "$(basename "$f")"
done
if [ "${#SKIPPED[@]}" -gt 0 ]; then
  echo "skipped:"
  printf '  %s\n' "${SKIPPED[@]}"
  if [ "$REQUIRE_ALL" = 1 ]; then fail "${#SKIPPED[@]} format(s) or check(s) skipped, and every one is required here"; fi
fi
